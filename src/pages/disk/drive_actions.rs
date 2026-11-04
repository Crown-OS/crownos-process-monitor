use std::sync::Arc;

use crownui::kit::{ButtonOptions, ButtonSize, ButtonVariant, Toasts, button, icons};
use crownui::prelude::{AnyView, Cx, Listeners, Signal, Store, View};
use monitor_engine::{Command, DriveAction, DriveOutcome};

use crate::app::EngineLink;
use crate::components::confirm::{Confirmation, confirm_dialog};
use crate::util::broadcast::follow_broadcast;

const NO_ENGINE: &str = "Drive actions need the monitoring engine";

const fn verb(action: DriveAction) -> &'static str {
    match action {
        DriveAction::Unmount => "Unmount",
        DriveAction::Eject => "Eject",
        DriveAction::PowerOff => "Power Off",
    }
}

const fn icon(action: DriveAction) -> &'static str {
    match action {
        DriveAction::Unmount | DriveAction::Eject => icons::HARD_DRIVE_UPLOAD,
        DriveAction::PowerOff => icons::POWER_OFF,
    }
}

const fn consequence(action: DriveAction) -> &'static str {
    match action {
        DriveAction::Unmount => "Programs using it lose access until it is mounted again.",
        DriveAction::Eject => "The medium is unmounted and ejected.",
        DriveAction::PowerOff => "The drive stops so it can be unplugged safely.",
    }
}

/// The toast after UDisks2 answered.
pub fn outcome_message(outcome: &DriveOutcome) -> String {
    let device = &outcome.block_device;
    match (&outcome.result, outcome.action) {
        (Ok(()), DriveAction::Unmount) => format!("{device} unmounted"),
        (Ok(()), DriveAction::Eject) => format!("{device} ejected"),
        (Ok(()), DriveAction::PowerOff) => format!("{device} powered off; unplug it now"),
        (Err(reason), action) => format!("Cannot {} {device}: {reason}", verb(action)),
    }
}

#[derive(Clone)]
struct DriveRequest {
    block_device: Arc<str>,
    action: DriveAction,
}

/// Unmount, Eject and Power Off: each asks first, runs through the engine's UDisks2 handle
/// (polkit may ask too) and ends with a toast.
#[derive(Clone, Copy)]
pub struct DriveActions {
    toasts: Toasts,
    open: Signal<bool>,
    confirmation: Confirmation,
    pending: Store<Option<DriveRequest>>,
}

impl DriveActions {
    pub fn new(cx: &mut Cx, toasts: Toasts) -> Self {
        if let Some(outcomes) = EngineLink::find(cx).and_then(|link| link.drive_outcomes()) {
            follow_broadcast(cx, outcomes, move |cx, outcome: DriveOutcome| {
                toasts.show(cx, outcome_message(&outcome));
            });
        }
        Self {
            toasts,
            open: cx.signal(false),
            confirmation: Confirmation::new(cx, "Unmount", None),
            pending: cx.store(None),
        }
    }

    /// A small button asking to apply `action` to `block_device`, shown as `name`.
    pub fn button(self, block_device: Arc<str>, name: Arc<str>, action: DriveAction) -> AnyView {
        button(verb(action))
            .variant(ButtonVariant::Secondary)
            .control_size(ButtonSize::Small)
            .leading_icon(icon(action))
            .on_click(move |cx, _| self.ask(cx, Arc::clone(&block_device), &name, action))
            .boxed()
    }

    fn ask(self, cx: &mut Cx, block_device: Arc<str>, name: &str, action: DriveAction) {
        let confirmation = self.confirmation;
        confirmation
            .title
            .set(cx, format!("{} {name}?", verb(action)));
        confirmation.message.set(cx, consequence(action).to_owned());
        confirmation.confirm_label.set(cx, verb(action));
        self.pending.update(cx, |pending| {
            *pending = Some(DriveRequest {
                block_device,
                action,
            });
        });
        self.open.set(cx, true);
    }

    pub fn view(self) -> impl View {
        confirm_dialog(self.open, self.confirmation, move |cx: &mut Cx, _| {
            let Some(request) = self.pending.update(cx, Option::take) else {
                return;
            };
            match EngineLink::find(cx) {
                Some(link) => link.send(Command::Drive {
                    block_device: request.block_device,
                    action: request.action,
                }),
                None => self.toasts.show(cx, NO_ENGINE),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcomes_read_as_toasts() {
        let done = DriveOutcome {
            block_device: "sdb1".into(),
            action: DriveAction::Unmount,
            result: Ok(()),
        };
        assert_eq!(outcome_message(&done), "sdb1 unmounted");
        let refused = DriveOutcome {
            action: DriveAction::PowerOff,
            result: Err("Not authorized".into()),
            ..done
        };
        assert_eq!(
            outcome_message(&refused),
            "Cannot Power Off sdb1: Not authorized"
        );
    }
}
