use monitor_core::ProcessKey;
use monitor_engine::Snapshot;
use monitor_engine::system::{
    ActionError, CpuMask, ProcessAction, perform, perform_app, renice, set_affinity,
};

use super::target::{ActionTarget, process_count};

/// What the user picked from the menu or the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowAction {
    Signal(ProcessAction),
    Renice(i32),
    Affinity(CpuMask),
}

impl RowAction {
    pub const fn verb(self) -> &'static str {
        match self {
            Self::Signal(ProcessAction::Quit) => "Quit",
            Self::Signal(ProcessAction::ForceQuit) => "Force Quit",
            Self::Signal(ProcessAction::Pause) => "Pause",
            Self::Signal(ProcessAction::Resume) => "Resume",
            Self::Renice(_) => "Renice",
            Self::Affinity(_) => "Set CPU Affinity",
        }
    }

    pub const fn past_tense(self) -> &'static str {
        match self {
            Self::Signal(ProcessAction::Quit) => "asked to quit",
            Self::Signal(ProcessAction::ForceQuit) => "force quit",
            Self::Signal(ProcessAction::Pause) => "paused",
            Self::Signal(ProcessAction::Resume) => "resumed",
            Self::Renice(_) => "reniced",
            Self::Affinity(_) => "moved to the chosen CPUs",
        }
    }
}

/// Signals, freezes, renices or pins the target. Touches `/proc` and cgroup files: run it on a
/// blocking task. Returns the processes reached.
pub fn run(
    snapshot: &Snapshot,
    target: &ActionTarget,
    action: RowAction,
) -> Result<usize, ActionError> {
    let table = &snapshot.processes;
    match (target, action) {
        (ActionTarget::Process { key, .. }, RowAction::Signal(signal)) => {
            perform(*key, signal).map(|()| 1)
        }
        (ActionTarget::Process { key, .. }, action) => apply_to(*key, action).map(|()| 1),
        (ActionTarget::App { key, .. }, RowAction::Signal(signal)) => {
            let app = table.app_index(key).ok_or(ActionError::Gone)?;
            perform_app(table, app, signal)
        }
        (ActionTarget::App { key, .. }, action) => {
            let app = table.app_index(key).ok_or(ActionError::Gone)?;
            apply_to_members(table.processes_of(app).map(|row| row.key), action)
        }
    }
}

fn apply_to(key: ProcessKey, action: RowAction) -> Result<(), ActionError> {
    match action {
        RowAction::Signal(signal) => perform(key, signal),
        RowAction::Renice(nice) => renice(key, nice),
        RowAction::Affinity(cpus) => set_affinity(key, &cpus),
    }
}

fn apply_to_members(
    members: impl Iterator<Item = ProcessKey>,
    action: RowAction,
) -> Result<usize, ActionError> {
    let mut reached = 0;
    let mut first_error = None;
    for key in members {
        match apply_to(key, action) {
            Ok(()) => reached += 1,
            Err(ActionError::Gone) => {}
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    match first_error {
        Some(error) if reached == 0 => Err(error),
        _ => Ok(reached),
    }
}

/// The toast after an action: what happened, or why it did not.
pub fn outcome_message(
    target: &ActionTarget,
    action: RowAction,
    result: &Result<usize, ActionError>,
) -> String {
    let name = target.name();
    match result {
        Ok(reached) if target.is_app() => {
            format!(
                "{name}: {} {}",
                process_count(*reached),
                action.past_tense()
            )
        }
        Ok(_) => format!("{name} {}", action.past_tense()),
        Err(error) => format!("{name}: {}", error_text(error)),
    }
}

fn error_text(error: &ActionError) -> String {
    match error {
        ActionError::PermissionDenied => {
            "needs the helper (another user's process or higher priority)".to_owned()
        }
        ActionError::Gone => "the process has already exited".to_owned(),
        ActionError::NoCpus => "choose at least one CPU".to_owned(),
        ActionError::Refused(reason) => reason.explanation().to_owned(),
        ActionError::Io(error) => error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};

    use monitor_core::{Pid, ProcessKey};

    use super::*;

    fn start_ticks(pid: u32) -> Option<u64> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let after_name = stat.rsplit_once(')')?.1;
        after_name.split_whitespace().nth(19)?.parse().ok()
    }

    #[test]
    fn permission_errors_point_at_the_helper() {
        let target = ActionTarget::Process {
            key: ProcessKey {
                pid: Pid::new(1),
                start_ticks: 0,
            },
            name: "sshd".into(),
        };
        let message = outcome_message(
            &target,
            RowAction::Signal(ProcessAction::Quit),
            &Err(ActionError::PermissionDenied),
        );
        assert!(message.starts_with("sshd: needs the helper"), "{message}");
    }

    #[test]
    fn affinity_outcomes_read_as_sentences() {
        let target = ActionTarget::Process {
            key: ProcessKey {
                pid: Pid::new(1234),
                start_ticks: 0,
            },
            name: "ffmpeg".into(),
        };
        let pin = RowAction::Affinity(CpuMask::from_cpus([2, 3]));
        assert_eq!(
            outcome_message(&target, pin, &Ok(1)),
            "ffmpeg moved to the chosen CPUs"
        );
        assert_eq!(
            outcome_message(&target, pin, &Err(ActionError::NoCpus)),
            "ffmpeg: choose at least one CPU"
        );
    }

    #[test]
    fn app_results_count_the_processes() {
        let target = ActionTarget::App {
            key: monitor_core::AppKey::new(monitor_core::AppKind::Desktop, "firefox"),
            name: "Firefox".into(),
            listed: 96,
            pausable: true,
        };
        let message = outcome_message(&target, RowAction::Signal(ProcessAction::Pause), &Ok(96));
        assert_eq!(message, "Firefox: 96 processes paused");
    }

    #[test]
    fn quitting_a_child_we_spawned_ends_it() {
        let Ok(mut child) = Command::new("sleep")
            .arg("30")
            .stdout(Stdio::null())
            .spawn()
        else {
            return;
        };
        let pid = child.id();
        let Some(start_ticks) = start_ticks(pid) else {
            let _ = child.kill();
            return;
        };
        let target = ActionTarget::Process {
            key: ProcessKey {
                pid: Pid::new(pid),
                start_ticks,
            },
            name: "sleep".into(),
        };
        let result = run(
            &Snapshot::default(),
            &target,
            RowAction::Signal(ProcessAction::Quit),
        );
        assert!(result.is_ok(), "{result:?}");
        let status = child.wait();
        assert!(status.is_ok_and(|status| !status.success()));
    }
}
