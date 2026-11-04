//! Keeps the engine's alert settings in step with the Insights settings: limits and custom
//! limits through `SetSettings`, popups through `SetNotifications`, quiet rules through
//! `Quiet` / `Unquiet`. Table menu picks
//! ("Alert when…", "Don't alert about…") become saved rules, editable in Settings.

use crownui::prelude::Cx;
use monitor_core::util::time::unix_now;
use monitor_engine::{Command, EngineControl, EngineSettings};

use super::settings::engine_settings;
use crate::app::AppState;
use crate::process_table::{AlertRequest, AlertWish};
use crate::settings::{InsightSettings, LimitValue, QuietTarget, SavedApp, SavedLimit, SavedQuiet};

/// "Alert when…" starts as CPU above half the machine for five minutes.
const PRESET_CPU_PERCENT: f32 = 50.0;
const PRESET_MINUTES: u32 = 5;

pub fn follow(cx: &mut Cx, state: AppState, engine: EngineControl) {
    let settings = state.settings;
    take_alert_requests(cx, state);
    let cadence = settings.field(cx, |settings| {
        (settings.sampling, settings.retention, settings.alerts)
    });
    let insights = settings.insights();
    let custom_limits =
        cx.memo(move |runtime| insights.with(runtime, InsightSettings::engine_limits));
    let notifications = settings.field(cx, |settings| settings.alerts.notifications);
    let toggles = engine.clone();
    cx.effect(move |runtime| {
        let mode = notifications.get(runtime).engine();
        let _ = toggles.send(Command::SetNotifications(mode));
    });
    let commands = engine.clone();
    cx.effect(move |runtime| {
        let (sampling, retention, alerts) = cadence.get(runtime);
        let custom_limits = custom_limits.with(runtime, Clone::clone);
        let _ = commands.send(Command::SetSettings(EngineSettings {
            thresholds: alerts.thresholds(),
            custom_limits,
            ..engine_settings(sampling, retention)
        }));
    });
    let mut sent: Vec<SavedQuiet> = Vec::new();
    cx.effect(move |runtime| {
        let now = unix_now();
        let wanted: Vec<SavedQuiet> = insights.with(runtime, |insights| {
            insights.active_quiets(now).cloned().collect()
        });
        for command in quiet_changes(&sent, &wanted) {
            let _ = engine.send(command);
        }
        sent = wanted;
    });
}

fn take_alert_requests(cx: &mut Cx, state: AppState) {
    let (requests, insights) = (state.alert_request, state.settings.insights());
    cx.effect(move |runtime| {
        let Some(request) = requests.get(runtime) else {
            return;
        };
        requests.set(runtime, None);
        insights.update(runtime, |insights| apply_request(insights, &request));
    });
}

fn apply_request(insights: &mut InsightSettings, request: &AlertRequest) {
    let Some(key) = &request.app else {
        return;
    };
    let app = SavedApp::new(key, &request.name);
    match request.wish {
        AlertWish::DontAlert => insights.quiet(QuietTarget::App(app), None),
        AlertWish::AlertWhen => {
            insights.custom_limits.retain(|limit| {
                limit.app.key != app.key || !matches!(limit.value, LimitValue::CpuPercent(_))
            });
            insights.custom_limits.push(SavedLimit {
                app,
                value: LimitValue::CpuPercent(PRESET_CPU_PERCENT),
                minutes: PRESET_MINUTES,
            });
        }
    }
}

/// The commands that turn the rules the engine has into the wanted ones.
fn quiet_changes(sent: &[SavedQuiet], wanted: &[SavedQuiet]) -> Vec<Command> {
    let dropped = sent
        .iter()
        .filter(|rule| !wanted.iter().any(|wanted| wanted.target == rule.target))
        .filter_map(|rule| rule.target.scope())
        .map(Command::Unquiet);
    let added = wanted
        .iter()
        .filter(|rule| !sent.contains(rule))
        .filter_map(|rule| {
            Some(Command::Quiet {
                scope: rule.target.scope()?,
                until: rule.until(),
            })
        });
    dropped.chain(added).collect()
}

#[cfg(test)]
mod tests {
    use monitor_engine::alerts::AlertKind;

    use super::*;
    use crate::settings::QuietTarget;

    #[test]
    fn table_menu_picks_become_rules() {
        let request = |wish| AlertRequest {
            wish,
            app: Some(monitor_core::AppKey::new(
                monitor_core::AppKind::Desktop,
                "steam",
            )),
            process: None,
            name: "Steam".into(),
        };
        let mut insights = InsightSettings::default();
        apply_request(&mut insights, &request(AlertWish::AlertWhen));
        apply_request(&mut insights, &request(AlertWish::AlertWhen));
        assert_eq!(
            insights.custom_limits.len(),
            1,
            "a repeat replaces the preset"
        );
        apply_request(&mut insights, &request(AlertWish::DontAlert));
        assert_eq!(insights.active_quiets(0).count(), 1);
    }

    #[test]
    fn only_changed_rules_are_sent() {
        let hot = SavedQuiet {
            target: QuietTarget::kind(AlertKind::MachineHot),
            until_unix: None,
        };
        let gpu = SavedQuiet {
            target: QuietTarget::kind(AlertKind::GpuBusy),
            until_unix: Some(10),
        };
        let first = quiet_changes(&[], std::slice::from_ref(&hot));
        assert!(matches!(first.as_slice(), [Command::Quiet { .. }]));
        assert!(quiet_changes(std::slice::from_ref(&hot), std::slice::from_ref(&hot)).is_empty());
        let swapped = quiet_changes(std::slice::from_ref(&hot), std::slice::from_ref(&gpu));
        assert!(matches!(
            swapped.as_slice(),
            [Command::Unquiet(_), Command::Quiet { .. }]
        ));
    }
}
