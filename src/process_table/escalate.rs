//! Actions refused for lack of permission (another user's process, raising priority) go to the
//! privileged helper when it serves them; its answers come back as toasts.

use monitor_engine::helper::{Capability, SignalKind};
use monitor_engine::system::{ActionError, ProcessAction};
use monitor_engine::{Command, HelperAction, HelperFailure, HelperOutcome, Snapshot};

use super::perform::RowAction;
use super::target::ActionTarget;

const fn signal_kind(action: ProcessAction) -> SignalKind {
    match action {
        ProcessAction::Quit => SignalKind::Term,
        ProcessAction::ForceQuit => SignalKind::Kill,
        ProcessAction::Pause => SignalKind::Stop,
        ProcessAction::Resume => SignalKind::Cont,
    }
}

/// The helper actions to retry `result` with: every process of the target, when the local
/// attempt lacked permission and the helper may signal other users' processes.
pub fn helper_actions(
    snapshot: &Snapshot,
    target: &ActionTarget,
    action: RowAction,
    result: &Result<usize, ActionError>,
) -> Vec<HelperAction> {
    let denied = matches!(result, Err(ActionError::PermissionDenied));
    if !denied || !snapshot.helper.serves(Capability::SignalOtherUsers) {
        return Vec::new();
    }
    let table = &snapshot.processes;
    let keys: Vec<_> = match target {
        ActionTarget::Process { key, .. } => vec![*key],
        ActionTarget::App { key, .. } => table
            .app_index(key)
            .map(|app| table.processes_of(app).map(|row| row.key).collect())
            .unwrap_or_default(),
    };
    keys.into_iter()
        .filter_map(|key| match action {
            RowAction::Signal(signal) => Some(HelperAction::Signal {
                key,
                signal: signal_kind(signal),
            }),
            RowAction::Renice(nice) => Some(HelperAction::Renice { key, nice }),
            RowAction::Affinity(_) => None,
        })
        .collect()
}

pub fn command(action: HelperAction) -> Command {
    match action {
        HelperAction::Signal { key, signal } => Command::HelperSignal { key, signal },
        HelperAction::Renice { key, nice } => Command::HelperRenice { key, nice },
        HelperAction::SetFan { fan, percent } => Command::SetFan { fan, percent },
        HelperAction::RestoreFans => Command::RestoreFans,
    }
}

/// The toast once the helper answered for `name`.
pub fn helper_message(name: &str, action: RowAction, outcome: &HelperOutcome) -> String {
    match &outcome.result {
        Ok(()) => format!("{name} {} through the helper", action.past_tense()),
        Err(HelperFailure::Unavailable) => format!("{name}: the helper is not available"),
        Err(HelperFailure::Denied(reason) | HelperFailure::Failed(reason)) => {
            format!("{name}: {reason}")
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use monitor_core::{Pid, ProcessKey};
    use monitor_engine::HelperStatus;
    use monitor_engine::helper::HelperAvailability;

    use super::*;

    fn target() -> ActionTarget {
        ActionTarget::Process {
            key: ProcessKey {
                pid: Pid::new(1234),
                start_ticks: 5,
            },
            name: "postgres".into(),
        }
    }

    #[test]
    fn denied_actions_go_to_a_helper_that_signals_other_users() {
        let denied = Err(ActionError::PermissionDenied);
        let quit = RowAction::Signal(ProcessAction::Quit);
        let without = Snapshot::default();
        assert!(helper_actions(&without, &target(), quit, &denied).is_empty());
        let with = Snapshot {
            helper: HelperStatus {
                availability: HelperAvailability::Available,
                capabilities: Arc::from([Capability::SignalOtherUsers]),
            },
            ..Snapshot::default()
        };
        assert!(matches!(
            helper_actions(&with, &target(), quit, &denied).as_slice(),
            [HelperAction::Signal {
                signal: SignalKind::Term,
                ..
            }]
        ));
        assert!(helper_actions(&with, &target(), quit, &Ok(1)).is_empty());
        let renice = helper_actions(&with, &target(), RowAction::Renice(-5), &denied);
        assert!(matches!(
            renice.as_slice(),
            [HelperAction::Renice { nice: -5, .. }]
        ));
        let pin = RowAction::Affinity(monitor_engine::system::CpuMask::from_cpus([0]));
        assert!(
            helper_actions(&with, &target(), pin, &denied).is_empty(),
            "the helper cannot set affinity, so the toast says it is needed"
        );
        let outcome = HelperOutcome {
            action: renice[0].clone(),
            result: Ok(()),
        };
        assert_eq!(
            helper_message("postgres", RowAction::Renice(-5), &outcome),
            "postgres reniced through the helper"
        );
    }
}
