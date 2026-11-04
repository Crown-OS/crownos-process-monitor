use monitor_core::ProcessKey;
use rustix::process::{PidfdFlags, Signal, pidfd_open, pidfd_send_signal, setpriority_process};

use super::verdict::{ProcessProbe, Verdict, verdict_for};
use super::{ActionError, ProcessAction};

impl ProcessAction {
    pub(crate) fn signal(self) -> Signal {
        match self {
            Self::Quit => Signal::TERM,
            Self::ForceQuit => Signal::KILL,
            Self::Pause => Signal::STOP,
            Self::Resume => Signal::CONT,
        }
    }
}

pub(super) fn system_pid(key: ProcessKey) -> Result<rustix::process::Pid, ActionError> {
    i32::try_from(key.pid.get())
        .ok()
        .and_then(rustix::process::Pid::from_raw)
        .ok_or(ActionError::Gone)
}

/// Signals `key` through a pidfd. The start time is checked after the pidfd
/// is open, so a reused PID can never receive the signal.
pub fn perform(key: ProcessKey, action: ProcessAction) -> Result<(), ActionError> {
    let pidfd = pidfd_open(system_pid(key)?, PidfdFlags::empty())?;
    let probe = ProcessProbe::read(key).ok_or(ActionError::Gone)?;
    if let Verdict::Refuse(reason) = verdict_for(&probe, key.pid, action) {
        return Err(ActionError::Refused(reason));
    }
    pidfd_send_signal(&pidfd, action.signal())?;
    Ok(())
}

/// Sets the nice value. Lowering it below the current value needs
/// `CAP_SYS_NICE` and reports [`ActionError::PermissionDenied`].
pub fn renice(key: ProcessKey, nice: i32) -> Result<(), ActionError> {
    let pid = system_pid(key)?;
    let probe = ProcessProbe::read(key).ok_or(ActionError::Gone)?;
    if probe.stat.is_kernel_thread() {
        return Err(ActionError::Refused(super::RefuseReason::KernelThread));
    }
    setpriority_process(Some(pid), nice)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use monitor_core::Pid;

    use super::*;
    use crate::process::parse_stat;

    fn key_and_nice(pid: u32) -> Option<(ProcessKey, i64)> {
        let text = std::fs::read(format!("/proc/{pid}/stat")).ok()?;
        parse_stat(&text).map(|(_, stat)| {
            (
                ProcessKey {
                    pid: Pid::new(pid),
                    start_ticks: stat.start_ticks,
                },
                stat.nice,
            )
        })
    }

    #[test]
    fn rejects_stale_keys_and_own_process() {
        let Some((own, nice)) = key_and_nice(std::process::id()) else {
            return;
        };
        let stale = ProcessKey {
            start_ticks: own.start_ticks + 1,
            ..own
        };
        assert!(matches!(
            perform(stale, ProcessAction::ForceQuit),
            Err(ActionError::Gone)
        ));
        assert!(matches!(
            perform(own, ProcessAction::Pause),
            Err(ActionError::Refused(_))
        ));
        assert!(
            renice(own, i32::try_from(nice).unwrap_or(0)).is_ok(),
            "same nice value is allowed"
        );
    }

    #[test]
    fn refuses_init() {
        let Some((init, _)) = key_and_nice(1) else {
            return;
        };
        assert!(matches!(
            perform(init, ProcessAction::Quit),
            Err(ActionError::Refused(_))
        ));
    }
}
