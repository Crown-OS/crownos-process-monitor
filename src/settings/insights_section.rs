//! The Insights section of the settings dialog: alert limits, per-app limits, quieted
//! alerts and deleting history.

mod delete;
mod limits;
mod quiet;

use crownui::kit::section;
use crownui::prelude::{Cx, View};

use super::dialog::choice_row;
use super::store::Settings;

pub fn insights_section(cx: &mut Cx, settings: Settings) -> impl View + use<> {
    section(
        "Insights",
        (
            choice_row(
                cx,
                settings,
                "Busy CPU above",
                |s| s.alerts.sustained_cpu,
                |s, v| {
                    s.alerts.sustained_cpu = v;
                },
            ),
            choice_row(
                cx,
                settings,
                "Busy for",
                |s| s.alerts.sustained_minutes,
                |s, v| {
                    s.alerts.sustained_minutes = v;
                },
            ),
            choice_row(
                cx,
                settings,
                "Memory growth in an hour",
                |s| s.alerts.memory_growth,
                |s, v| {
                    s.alerts.memory_growth = v;
                },
            ),
            choice_row(
                cx,
                settings,
                "Running hot at",
                |s| s.alerts.heat,
                |s, v| {
                    s.alerts.heat = v;
                },
            ),
            choice_row(
                cx,
                settings,
                "Busy GPU above",
                |s| s.alerts.busy_gpu,
                |s, v| {
                    s.alerts.busy_gpu = v;
                },
            ),
            choice_row(
                cx,
                settings,
                "Notifications",
                |s| s.alerts.notifications,
                |s, v| {
                    s.alerts.notifications = v;
                },
            ),
            limits::custom_limits(cx, settings.insights()),
            quiet::quieted_alerts(settings.insights()),
            delete::delete_history(cx),
        ),
    )
}

#[cfg(test)]
mod tests {
    use crowntest::{Role, TestApp, role};
    use crownui::kit::{Theme, provide_theme};
    use crownui::prelude::component;
    use monitor_engine::alerts::AlertKind;

    use crate::settings::{MonitorSettings, QuietTarget, Settings, settings_dialog};

    fn dialog() -> TestApp {
        TestApp::with_size(
            component(|cx| {
                provide_theme(cx, Theme::dark());
                let settings = Settings::new(cx, MonitorSettings::default(), None);
                settings.insights().update(cx, |insights| {
                    insights.quiet(QuietTarget::kind(AlertKind::MachineHot), None);
                });
                let open = cx.signal(true);
                settings_dialog(open, settings)
            }),
            900.0,
            2400.0,
        )
    }

    fn shows(app: &mut TestApp, wanted: &str) -> bool {
        app.ui()
            .semantic_tree()
            .iter()
            .any(|node| node.name.as_deref() == Some(wanted))
    }

    #[test]
    fn quieted_alerts_can_be_heard_again() {
        let mut app = dialog();
        app.settle();
        for wanted in [
            "Insights",
            "Tell me when…",
            "Quieted",
            "Running hot",
            "Always",
        ] {
            assert!(
                shows(&mut app, wanted),
                "{wanted} is missing:\n{}",
                app.semantics_snapshot()
            );
        }
        app.click(role(Role::Button).name("Tell me again"));
        app.settle();
        assert!(shows(&mut app, "No quieted alerts"));
    }

    #[test]
    fn deleting_history_asks_first() {
        let mut app = dialog();
        app.settle();
        app.click(role(Role::Button).name("Delete History…"));
        app.settle();
        assert!(app.exists(role(Role::Button).name("Delete")));
        app.click(role(Role::Button).name("Delete"));
        app.settle();
        assert!(shows(&mut app, "History deleted"));
    }

    #[test]
    fn an_unparsable_limit_explains_itself() {
        let mut app = dialog();
        app.settle();
        app.click(role(Role::Button).name("Add limit"));
        app.settle();
        assert!(shows(
            &mut app,
            "Pick an app and type a number (percent, or GB of memory)"
        ));
    }
}
