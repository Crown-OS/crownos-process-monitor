use crownui::kit::{ButtonOptions, ButtonVariant, button, use_theme};
use crownui::prelude::{
    Listeners, Signal, Styled, View, bind, component, hstack, keyed, spacer, text, vstack,
};
use monitor_core::util::time::{local_day, unix_now};
use monitor_engine::alerts::AlertKind;
use monitor_engine::utc_offset_seconds;

use crate::settings::insights::{InsightSettings, QuietTarget, SavedQuiet};
use crate::util::alert_text::kind_label;
use crate::util::time::{write_clock, write_date};

const ROW_GAP: f32 = 8.0;
const CAPTION_SIZE: f32 = 12.0;

/// "Firefox", or the kind for machine-wide rules.
pub fn quiet_target_label(target: &QuietTarget) -> String {
    match target {
        QuietTarget::App(app) => app.name.clone(),
        QuietTarget::Kind(code) => match AlertKind::from_code(code) {
            Some(kind) => kind_label(kind).to_owned(),
            None => code.clone(),
        },
    }
}

/// "Always", or "Until Tomorrow 00:00".
pub fn quiet_until_label(rule: &SavedQuiet, now_unix: i64, utc_offset_seconds: i32) -> String {
    let Some(until) = rule.until_unix else {
        return "Always".to_owned();
    };
    let mut out = String::from("Until ");
    match local_day(until, utc_offset_seconds) - local_day(now_unix, utc_offset_seconds) {
        0 => out.push_str("today"),
        1 => out.push_str("tomorrow"),
        _ => write_date(&mut out, until, utc_offset_seconds),
    }
    out.push(' ');
    write_clock(&mut out, until, utc_offset_seconds);
    out
}

/// The quieted apps and kinds, each with "Tell me again".
pub fn quieted_alerts(insights: Signal<InsightSettings>) -> impl View {
    component(move |cx| {
        let theme = use_theme(cx);
        let muted = move || bind(move |runtime| theme.with(runtime, |theme| theme.text.muted));
        let list = keyed(
            move |runtime| {
                let now = unix_now();
                insights.with(runtime, |insights| {
                    insights.active_quiets(now).cloned().collect::<Vec<_>>()
                })
            },
            move |_, rules| {
                if rules.is_empty() {
                    return text("No quieted alerts")
                        .text_size(CAPTION_SIZE)
                        .text_color(muted())
                        .boxed();
                }
                let (now, offset) = (unix_now(), utc_offset_seconds());
                let rows: Vec<_> = rules
                    .into_iter()
                    .map(|rule| {
                        let until = quiet_until_label(&rule, now, offset);
                        let target = rule.target;
                        hstack((
                            text(quiet_target_label(&target)).text_size(CAPTION_SIZE),
                            text(until).text_size(CAPTION_SIZE).text_color(muted()),
                            spacer(),
                            button("Tell me again")
                                .variant(ButtonVariant::Ghost)
                                .on_click(move |cx, _| {
                                    insights.update(cx, |insights| insights.unquiet(&target));
                                }),
                        ))
                        .gap(ROW_GAP)
                        .items_center()
                    })
                    .collect();
                vstack(rows).gap(ROW_GAP).boxed()
            },
        );
        vstack((text("Quieted").font_semibold(), list))
            .gap(ROW_GAP)
            .py(ROW_GAP)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::insights::SavedApp;

    const MIDNIGHT: i64 = 1_793_750_400;

    #[test]
    fn quiet_rules_describe_what_and_until_when() {
        let app = QuietTarget::App(SavedApp {
            key: "desktop:firefox".to_owned(),
            name: "Firefox".to_owned(),
        });
        assert_eq!(quiet_target_label(&app), "Firefox");
        assert_eq!(
            quiet_target_label(&QuietTarget::kind(AlertKind::MachineHot)),
            "Running hot"
        );
        let forever = SavedQuiet {
            target: app.clone(),
            until_unix: None,
        };
        assert_eq!(quiet_until_label(&forever, MIDNIGHT, 0), "Always");
        let tomorrow = SavedQuiet {
            target: app,
            until_unix: Some(MIDNIGHT + 86_400),
        };
        assert_eq!(
            quiet_until_label(&tomorrow, MIDNIGHT + 3_600, 0),
            "Until tomorrow 00:00"
        );
    }
}
