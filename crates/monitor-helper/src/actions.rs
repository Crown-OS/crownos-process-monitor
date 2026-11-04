//! Signals and nice values for any user's process. Both go through
//! `monitor-system`'s refuse list and are pinned to `(pid, start_ticks)`.

use std::os::fd::OwnedFd;

use monitor_core::{Pid, ProcessKey};
use monitor_system::{ProcessAction, Verdict};
use rustix::event::{PollFd, PollFlags, Timespec};
use rustix::process::{PidfdFlags, pidfd_open};

use crate::{HelperError, SignalKind};

const NICE_RANGE: std::ops::RangeInclusive<i32> = -20..=19;

fn process_key(pid: u32, start_ticks: u64) -> Result<ProcessKey, HelperError> {
    if pid == 0 || start_ticks == 0 {
        return Err(HelperError::invalid("a process needs a pid and start time"));
    }
    Ok(ProcessKey {
        pid: Pid::new(pid),
        start_ticks,
    })
}

fn validate_nice(nice: i32) -> Result<i32, HelperError> {
    if NICE_RANGE.contains(&nice) {
        Ok(nice)
    } else {
        Err(HelperError::invalid(format!(
            "nice {nice} is outside -20..=19"
        )))
    }
}

pub fn signal(pid: u32, start_ticks: u64, signal: &str) -> Result<(), HelperError> {
    let kind = SignalKind::parse(signal)
        .ok_or_else(|| HelperError::invalid(format!("unknown signal {signal:?}")))?;
    let key = process_key(pid, start_ticks)?;
    monitor_system::perform(key, kind.action()).map_err(HelperError::from)
}

/// Opens a pidfd before checking the start time and verifies afterwards
/// that the same process is still alive, so a PID recycled in between can
/// never be reniced unnoticed.
pub fn renice(pid: u32, start_ticks: u64, nice: i32) -> Result<(), HelperError> {
    let nice = validate_nice(nice)?;
    let key = process_key(pid, start_ticks)?;
    let pidfd = open_pidfd(key)?;
    if let Verdict::Refuse(reason) = monitor_system::verdict(key, ProcessAction::Quit) {
        return Err(HelperError::Refused(reason.explanation().to_owned()));
    }
    monitor_system::renice(key, nice)?;
    if has_exited(&pidfd) {
        return Err(HelperError::Gone(
            "the process exited while being reniced".to_owned(),
        ));
    }
    Ok(())
}

fn open_pidfd(key: ProcessKey) -> Result<OwnedFd, HelperError> {
    let gone = || HelperError::Gone(format!("process {} has exited", key.pid));
    let pid = i32::try_from(key.pid.get())
        .ok()
        .and_then(rustix::process::Pid::from_raw)
        .ok_or_else(gone)?;
    pidfd_open(pid, PidfdFlags::empty()).map_err(|_| gone())
}

/// A pidfd polls readable once its process exited.
fn has_exited(pidfd: &OwnedFd) -> bool {
    let mut poll = [PollFd::new(pidfd, PollFlags::IN)];
    let no_wait = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    rustix::event::poll(&mut poll, Some(&no_wait)).map_or(true, |ready| {
        ready > 0 && poll[0].revents().contains(PollFlags::IN)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_system::parse_stat;

    fn own_start_ticks() -> Option<u64> {
        let stat = std::fs::read("/proc/self/stat").ok()?;
        parse_stat(&stat).map(|(_, stat)| stat.start_ticks)
    }

    #[test]
    fn validates_arguments() {
        assert!(process_key(0, 5).is_err());
        assert!(process_key(5, 0).is_err());
        assert!(process_key(5, 5).is_ok());
        assert!(validate_nice(-20).is_ok());
        assert!(validate_nice(19).is_ok());
        assert!(validate_nice(20).is_err());
        assert!(validate_nice(-21).is_err());
        assert!(matches!(
            signal(4242, 1, "hup"),
            Err(HelperError::InvalidArgument(_))
        ));
        assert!(matches!(
            renice(4242, 1, 40),
            Err(HelperError::InvalidArgument(_))
        ));
    }

    #[test]
    fn refuses_the_helper_itself_and_stale_keys() {
        let Some(start_ticks) = own_start_ticks() else {
            return;
        };
        let own = std::process::id();
        assert!(matches!(
            signal(own, start_ticks, "stop"),
            Err(HelperError::Refused(_))
        ));
        assert!(matches!(
            renice(own, start_ticks, 0),
            Err(HelperError::Refused(_))
        ));
        assert!(matches!(
            signal(own, start_ticks + 1, "kill"),
            Err(HelperError::Gone(_))
        ));
    }

    #[test]
    fn live_processes_have_not_exited() {
        let Some(start_ticks) = own_start_ticks() else {
            return;
        };
        let Ok(key) = process_key(std::process::id(), start_ticks) else {
            return;
        };
        assert!(open_pidfd(key).is_ok_and(|pidfd| !has_exited(&pidfd)));
    }
}
