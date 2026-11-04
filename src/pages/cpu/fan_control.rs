use crownui::kit::{ButtonOptions, ButtonVariant, SliderOptions, Toasts, button, slider};
use crownui::prelude::{
    Cx, Listeners, Memo, Runtime, Semantic, Signal, Styled, View, bind, hstack, text,
};
use monitor_engine::hardware::sensors::FanControlSupport;
use monitor_engine::helper::Capability;
use monitor_engine::{Command, HelperAction, HelperFailure, HelperOutcome};

use crate::app::EngineLink;
use crate::app::live::LiveSnapshot;
use crate::util::broadcast::follow_broadcast;
use crate::util::spacing;

pub const FAN_HELPER_NOTE: &str = "Fan control needs the helper";
/// Lower duties risk overheating; the slider stops here.
const MIN_FIXED_PERCENT: f32 = 30.0;
const MAX_PERCENT: f32 = 100.0;
const PERCENT_STEP: f32 = 5.0;
const INITIAL_PERCENT: f32 = 60.0;
const NOTE_SIZE: f32 = 12.0;
const SLIDER_WIDTH: f32 = 140.0;

/// The toast for a fan outcome; `None` for other helper actions.
pub fn fan_message(outcome: &HelperOutcome) -> Option<String> {
    let done = match &outcome.action {
        HelperAction::SetFan { fan, percent } => {
            format!("Fan {} held at {percent} %", fan.channel)
        }
        HelperAction::RestoreFans => "Fans are back on automatic control".to_owned(),
        HelperAction::Signal { .. } | HelperAction::Renice { .. } => return None,
    };
    Some(match &outcome.result {
        Ok(()) => done,
        Err(HelperFailure::Unavailable) => "The helper is not available".to_owned(),
        Err(HelperFailure::Denied(reason) | HelperFailure::Failed(reason)) => {
            format!("Fan control failed: {reason}")
        }
    })
}

fn send_fixed(cx: &mut Cx, live: Signal<LiveSnapshot>, percent: Signal<f32>) {
    let Some(link) = EngineLink::find(cx) else {
        return;
    };
    let duty = percent.get(cx).clamp(MIN_FIXED_PERCENT, MAX_PERCENT) as u8;
    let fans: Vec<_> = live.with_untracked(cx, |snapshot| {
        snapshot
            .tick
            .sensors
            .fans
            .iter()
            .filter(|fan| fan.identity.control == FanControlSupport::Pwm)
            .map(|fan| fan.identity.id.clone())
            .collect()
    });
    for fan in fans {
        link.send(Command::SetFan { fan, percent: duty });
    }
}

/// Automatic, or a fixed duty for every PWM fan, through the helper; disabled until the helper
/// can write PWM. The helper restores automatic control when the app exits.
pub fn fan_control(cx: &mut Cx, live: Signal<LiveSnapshot>, toasts: Toasts) -> impl View + use<> {
    if let Some(outcomes) = EngineLink::find(cx).and_then(|link| link.helper_outcomes()) {
        follow_broadcast(cx, outcomes, move |cx, outcome: HelperOutcome| {
            if let Some(message) = fan_message(&outcome) {
                toasts.show(cx, message);
            }
        });
    }
    let ready: Memo<bool> = cx.memo(move |runtime| {
        live.with(runtime, |snapshot| {
            snapshot.tick.helper.serves(Capability::ControlFans)
        })
    });
    let blocked = move |runtime: &mut Runtime| !ready.get(runtime);
    let percent = cx.signal(INITIAL_PERCENT);
    hstack((
        button("Automatic")
            .variant(ButtonVariant::Secondary)
            .disabled(bind(blocked))
            .on_click(move |cx, _| {
                if let Some(link) = EngineLink::find(cx) {
                    link.send(Command::RestoreFans);
                }
            }),
        slider(percent)
            .range(MIN_FIXED_PERCENT, MAX_PERCENT)
            .step(PERCENT_STEP)
            .w(SLIDER_WIDTH),
        text(bind(move |runtime| {
            format!("{:.0} %", percent.get(runtime))
        }))
        .text_size(NOTE_SIZE),
        button("Fixed speed")
            .variant(ButtonVariant::Secondary)
            .disabled(bind(blocked))
            .on_click(move |cx, _| send_fixed(cx, live, percent)),
        text(bind(move |runtime| {
            if ready.get(runtime) {
                String::new()
            } else {
                FAN_HELPER_NOTE.to_owned()
            }
        }))
        .text_size(NOTE_SIZE),
    ))
    .gap(spacing::COMFORTABLE)
    .items_center()
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use monitor_engine::hardware::FanId;

    use super::*;

    #[test]
    fn fan_outcomes_become_toasts_and_others_do_not() {
        let fan = FanId {
            chip_directory: Arc::from(Path::new("/sys/class/hwmon/hwmon5")),
            channel: 1,
        };
        let held = HelperOutcome {
            action: HelperAction::SetFan { fan, percent: 60 },
            result: Ok(()),
        };
        assert_eq!(fan_message(&held).as_deref(), Some("Fan 1 held at 60 %"));
        let denied = HelperOutcome {
            result: Err(HelperFailure::Denied("dismissed".into())),
            ..held
        };
        assert_eq!(
            fan_message(&denied).as_deref(),
            Some("Fan control failed: dismissed")
        );
        let signal = HelperOutcome {
            action: HelperAction::Signal {
                key: monitor_core::ProcessKey {
                    pid: monitor_core::Pid::new(1),
                    start_ticks: 0,
                },
                signal: monitor_engine::helper::SignalKind::Term,
            },
            result: Ok(()),
        };
        assert!(fan_message(&signal).is_none());
    }
}
