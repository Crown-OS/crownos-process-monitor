mod collectors;
mod external;
mod schedule;

use std::sync::Arc;
use std::time::Instant;

use monitor_core::{Elapsed, Interest, ProcessKey, SourceResult, Tick};
use monitor_hardware::estimate_system_draw;
use monitor_io::UDisks;
use monitor_system::{ProcessDetails, cpu_info, memory_modules};
use tokio::sync::{broadcast, watch};

pub(crate) use self::collectors::Collectors;
pub(crate) use schedule::run;

use self::external::{TrafficTotals, join_external};
use crate::alerting::Alerting;
use crate::energy::EnergyEstimator;
use crate::privileged::{self, Privileged};
use crate::recording::Recorder;
use crate::{
    Command, Domain, DriveOutcome, EngineSettings, HelperAction, HelperConnection, HelperOutcome,
    Snapshot, drives,
};

/// Everything the sampler thread is started with.
pub(crate) struct Startup {
    pub interest: Interest,
    pub settings: EngineSettings,
    pub recorder: Option<Recorder>,
    pub history_failure: Option<String>,
    pub alerting: Alerting,
    pub helper: HelperConnection,
    pub outcomes: broadcast::Sender<HelperOutcome>,
    pub drive_outcomes: broadcast::Sender<DriveOutcome>,
}

/// Owns the collectors and the persistent snapshot they write into; each
/// tick publishes a clone of it.
pub(crate) struct Sampler {
    interest: Interest,
    settings: EngineSettings,
    collectors: Collectors,
    snapshot: Snapshot,
    previous_tick: Option<Tick>,
    last_sample: Option<Instant>,
    publisher: watch::Sender<Arc<Snapshot>>,
    recorder: Option<Recorder>,
    alerting: Alerting,
    udisks: Option<UDisks>,
    privileged: Option<Privileged>,
    outcomes: broadcast::Sender<HelperOutcome>,
    drive_outcomes: broadcast::Sender<DriveOutcome>,
    energy: EnergyEstimator,
    traffic_totals: TrafficTotals,
}

impl Sampler {
    pub fn new(startup: Startup, publisher: watch::Sender<Arc<Snapshot>>) -> Self {
        let mut snapshot = Snapshot::default();
        let collectors = Collectors::open(&mut snapshot.availability);
        if let Some(failure) = startup.history_failure {
            snapshot
                .availability
                .mark_unavailable(Domain::History, failure);
        }
        snapshot.cpu_info = Arc::new(cpu_info().unwrap_or_default());
        snapshot.memory_modules = memory_modules().into();
        let mut interest = startup.interest;
        interest.visible_interval = startup.settings.visible_interval;
        Self {
            interest,
            settings: startup.settings,
            udisks: collectors.udisks(),
            collectors,
            snapshot,
            previous_tick: None,
            last_sample: None,
            publisher,
            recorder: startup.recorder,
            alerting: startup.alerting,
            privileged: Privileged::connect(&startup.helper),
            outcomes: startup.outcomes,
            drive_outcomes: startup.drive_outcomes,
            energy: EnergyEstimator::default(),
            traffic_totals: TrafficTotals::default(),
        }
    }

    fn interval(&self) -> std::time::Duration {
        self.interest.sample_interval()
    }

    /// Returns whether anything a collector looks at changed.
    fn set_interest(&mut self, mut interest: Interest) -> bool {
        interest.visible_interval = self.settings.visible_interval;
        let changed = interest != self.interest;
        self.interest = interest;
        changed
    }

    fn sample(&mut self) {
        let started = Instant::now();
        let tick = Tick::now();
        let elapsed = self
            .previous_tick
            .map_or(Elapsed::default(), |previous| tick.since(&previous));
        self.previous_tick = Some(tick);
        self.last_sample = Some(started);

        let now = tick.monotonic();
        let snapshot = &mut self.snapshot;
        if let Some(helper) = self.privileged.as_mut() {
            helper.consume(&mut self.collectors, now);
            helper.request(&self.interest, &self.collectors, snapshot, &tick);
        }
        self.collectors
            .sample(tick, elapsed, &self.interest, snapshot);
        self.traffic_totals.update(snapshot);
        join_external(snapshot, &self.traffic_totals);
        if let Some(helper) = self.privileged.as_ref() {
            helper.apply_accounting(snapshot);
            helper.publish_status(snapshot);
        }
        snapshot.system_draw = estimate_system_draw(&snapshot.power, &snapshot.gpu);
        self.energy.estimate(snapshot);
        snapshot.sequence += 1;
        snapshot.tick = Some(tick);
        snapshot.elapsed = elapsed;
        snapshot.interest.clone_from(&self.interest);

        let recorder = self.recorder.as_ref();
        self.alerting.observe(snapshot, |alert| {
            if let Some(recorder) = recorder {
                recorder.save_alert(alert);
            }
        });
        self.alerting.publish_into(snapshot);
        if let Some(recorder) = self.recorder.as_mut() {
            recorder.record(snapshot);
        }
        snapshot.tick_cost = started.elapsed();
        self.publisher.send_replace(Arc::new(snapshot.clone()));
    }

    fn apply(&mut self, command: Command) {
        let recorder = self.recorder.as_ref();
        let persist = |alert| {
            if let Some(recorder) = recorder {
                recorder.save_alert(alert);
            }
        };
        match command {
            Command::SetSettings(settings) => {
                if let Some(recorder) = recorder
                    && settings.retention != self.settings.retention
                {
                    recorder.set_retention(settings.retention);
                }
                self.alerting
                    .configure(&settings.thresholds, &settings.custom_limits);
                self.interest.visible_interval = settings.visible_interval;
                self.settings = settings;
            }
            Command::SetNotifications(notifications) => {
                self.alerting.set_notifications(notifications);
            }
            Command::Quiet { scope, until } => self.alerting.quiet(scope, until, persist),
            Command::Unquiet(scope) => self.alerting.unquiet(&scope),
            Command::PinAlert { entry, pinned } => self.alerting.pin(entry, pinned, persist),
            Command::RefreshDriveHealth => {
                if let Some(udisks) = &self.udisks {
                    udisks.refresh_health();
                }
            }
            Command::Drive {
                block_device,
                action,
            } => drives::perform(
                self.udisks.as_ref(),
                block_device,
                action,
                &self.drive_outcomes,
            ),
            Command::DeleteHistory => {
                if let Some(recorder) = recorder {
                    recorder.delete_all();
                }
                self.alerting.clear();
            }
            Command::HelperSignal { key, signal } => {
                self.perform(HelperAction::Signal { key, signal });
            }
            Command::HelperRenice { key, nice } => {
                self.perform(HelperAction::Renice { key, nice });
            }
            Command::SetFan { fan, percent } => self.perform(HelperAction::SetFan { fan, percent }),
            Command::RestoreFans => self.perform(HelperAction::RestoreFans),
            Command::Shutdown => {}
        }
        self.alerting.publish_into(&mut self.snapshot);
    }

    fn inspect(&self, key: ProcessKey) -> SourceResult<ProcessDetails> {
        self.collectors.inspect(key)
    }

    fn perform(&self, action: HelperAction) {
        let client = self.privileged.as_ref().map(Privileged::client);
        privileged::perform(client, action, &self.outcomes);
    }

    fn finish(self) {
        if let Some(recorder) = self.recorder {
            recorder.finish(&self.snapshot);
        }
    }
}
