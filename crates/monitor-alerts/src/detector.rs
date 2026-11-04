use std::collections::HashMap;
use std::sync::Arc;

use monitor_core::AppKey;
use monitor_core::util::time::{MINUTE_SECONDS, align_down};

use crate::alert::{Alert, AlertApp};
use crate::cooldown::Cooldowns;
use crate::input::AlertInput;
use crate::quiet::{QuietList, QuietRule, QuietScope, QuietUntil};
use crate::rules::streak::Streak;
use crate::strain::StrainState;
use crate::thresholds::{AppLimit, CustomLimit, Thresholds};
use crate::tracker::{AppTracker, Candidate, MachineTracker};

/// Bounds rule state; apps beyond it are watched once others go idle.
const MAX_TRACKED_APPS: usize = 256;
/// Apps unseen this long are forgotten (their cooldowns are kept).
const IDLE_EVICTION_SECONDS: i64 = 10 * MINUTE_SECONDS;

/// Filters candidates through quieting and cooldowns into fired alerts.
struct Gate<'a> {
    cooldowns: &'a mut Cooldowns,
    quiet: &'a QuietList,
    cooldown_seconds: i64,
    at_unix: i64,
    fired: &'a mut Vec<Alert>,
}

impl Gate<'_> {
    fn offer(&mut self, candidate: Candidate, app: Option<(&AppKey, &Arc<str>)>) {
        let key = app.map(|(key, _)| key);
        if self.quiet.is_quiet(key, candidate.kind, self.at_unix)
            || !self
                .cooldowns
                .try_start(key, candidate.kind, self.at_unix, self.cooldown_seconds)
        {
            return;
        }
        self.fired.push(Alert {
            kind: candidate.kind,
            app: app.map(|(key, name)| AlertApp {
                key: key.clone(),
                name: Arc::clone(name),
            }),
            at_unix: self.at_unix,
            value: candidate.value,
            limit: candidate.limit,
        });
    }
}

/// Runs every alert rule on per-minute averages built from ticks. Pure
/// logic with bounded memory; alerts run only while the app is open.
#[derive(Debug)]
pub struct AlertDetector {
    thresholds: Thresholds,
    custom_limits: Vec<(CustomLimit, Streak)>,
    apps: HashMap<AppKey, AppTracker>,
    machine: MachineTracker,
    cooldowns: Cooldowns,
    quiet: QuietList,
    strain: StrainState,
    minute_unix: Option<i64>,
}

impl Default for AlertDetector {
    fn default() -> Self {
        Self::new(Thresholds::default())
    }
}

impl AlertDetector {
    pub fn new(thresholds: Thresholds) -> Self {
        Self {
            strain: StrainState::new(&thresholds),
            thresholds,
            custom_limits: Vec::new(),
            apps: HashMap::with_capacity(MAX_TRACKED_APPS),
            machine: MachineTracker::default(),
            cooldowns: Cooldowns::default(),
            quiet: QuietList::default(),
            minute_unix: None,
        }
    }

    pub fn thresholds(&self) -> &Thresholds {
        &self.thresholds
    }

    pub fn set_thresholds(&mut self, thresholds: Thresholds) {
        self.thresholds = thresholds;
    }

    pub fn set_custom_limits(&mut self, limits: impl IntoIterator<Item = CustomLimit>) {
        self.custom_limits = limits
            .into_iter()
            .map(|limit| (limit, Streak::default()))
            .collect();
    }

    pub fn quiet(&mut self, scope: QuietScope, until: QuietUntil) {
        self.quiet.quiet(scope, until);
    }

    pub fn unquiet(&mut self, scope: &QuietScope) {
        self.quiet.unquiet(scope);
    }

    /// Active quiet rules, for the engine to persist.
    pub fn quiet_rules(&self) -> &[QuietRule] {
        self.quiet.rules()
    }

    pub fn strain(&self) -> &StrainState {
        &self.strain
    }

    pub fn tracked_apps(&self) -> usize {
        self.apps.len()
    }

    /// Feeds one tick; alerts for a minute are appended to `fired` on the
    /// first tick of the next minute. `fired` is not cleared.
    pub fn observe(&mut self, input: &AlertInput<'_>, fired: &mut Vec<Alert>) {
        self.strain.update(input, &self.thresholds);
        let seconds = input.elapsed_seconds;
        if seconds <= 0.0 {
            return;
        }
        let minute = align_down(input.wall_unix_seconds, MINUTE_SECONDS);
        match self.minute_unix {
            Some(current) if current != minute => self.close_minute(current, fired),
            _ => {}
        }
        self.minute_unix = Some(minute);
        self.machine.add(input, seconds);
        for app in input.apps {
            if let Some(tracker) = self.apps.get_mut(&app.key) {
                tracker.add(app, seconds, minute);
            } else if self.apps.len() < MAX_TRACKED_APPS {
                let mut tracker = AppTracker::new(Arc::clone(&app.name), minute);
                tracker.add(app, seconds, minute);
                self.apps.insert(app.key.clone(), tracker);
            }
        }
    }

    fn close_minute(&mut self, minute_unix: i64, fired: &mut Vec<Alert>) {
        let at_unix = minute_unix + MINUTE_SECONDS;
        let thresholds = &self.thresholds;
        let mut gate = Gate {
            cooldowns: &mut self.cooldowns,
            quiet: &self.quiet,
            cooldown_seconds: thresholds.cooldown_seconds,
            at_unix,
            fired,
        };
        for candidate in self
            .machine
            .close_minute(minute_unix, thresholds)
            .into_iter()
            .flatten()
        {
            gate.offer(candidate, None);
        }
        for (key, tracker) in &mut self.apps {
            for candidate in tracker
                .close_minute(minute_unix, thresholds)
                .into_iter()
                .flatten()
            {
                gate.offer(candidate, Some((key, &tracker.name)));
            }
        }
        for (custom, streak) in &mut self.custom_limits {
            let Some((key, tracker)) = self.apps.get_key_value(&custom.app) else {
                streak.update(minute_unix, false);
                continue;
            };
            let observed = tracker
                .minute(minute_unix)
                .map(|minute| match custom.limit {
                    AppLimit::Memory(_) => minute.memory_bytes,
                    AppLimit::Cpu(_) => minute.cpu_percent,
                });
            let limit = custom.limit.value();
            let over = observed.is_some_and(|value| value > limit);
            if streak.update(minute_unix, over) >= custom.minutes.max(1) {
                let candidate = Candidate {
                    kind: custom.limit.kind(),
                    value: observed.unwrap_or(limit),
                    limit,
                };
                gate.offer(candidate, Some((key, &tracker.name)));
            }
        }
        self.apps.retain(|_, tracker| {
            minute_unix - tracker.last_seen_minute_unix < IDLE_EVICTION_SECONDS
        });
        self.cooldowns.prune(at_unix);
        self.quiet.prune(at_unix);
    }
}
