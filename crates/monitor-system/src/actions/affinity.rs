use monitor_core::ProcessKey;
use rustix::process::{PidfdFlags, pidfd_open};
use rustix::thread::{sched_getaffinity, sched_setaffinity};

use super::signal::system_pid;
use super::verdict::ProcessProbe;
use super::{ActionError, CpuMask};

/// The CPUs `key`'s main thread may run on.
pub fn affinity(key: ProcessKey) -> Result<CpuMask, ActionError> {
    let pid = system_pid(key)?;
    ProcessProbe::read(key).ok_or(ActionError::Gone)?;
    Ok(CpuMask::from_cpu_set(sched_getaffinity(Some(pid))?))
}

/// Pins every thread of `key` to `cpus`. The pidfd is opened and the start time checked
/// first, so a reused PID is never touched; the refuse list applies. Another user's
/// process reports [`ActionError::PermissionDenied`].
pub fn set_affinity(key: ProcessKey, cpus: &CpuMask) -> Result<(), ActionError> {
    if cpus.is_empty() {
        return Err(ActionError::NoCpus);
    }
    let pid = system_pid(key)?;
    let _pinned = pidfd_open(pid, PidfdFlags::empty())?;
    let probe = ProcessProbe::read(key).ok_or(ActionError::Gone)?;
    if let Some(reason) = probe.refuse_reason(key.pid) {
        return Err(ActionError::Refused(reason));
    }
    sched_setaffinity(Some(pid), cpus.as_cpu_set())?;
    let Ok(tasks) = std::fs::read_dir(format!("/proc/{}/task", key.pid)) else {
        return Ok(());
    };
    tasks
        .filter_map(|task| task.ok()?.file_name().to_str()?.parse::<i32>().ok())
        .filter(|&thread| thread != pid.as_raw_nonzero().get())
        .filter_map(rustix::process::Pid::from_raw)
        .try_for_each(
            |thread| match sched_setaffinity(Some(thread), cpus.as_cpu_set()) {
                Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
                Err(errno) => Err(errno.into()),
            },
        )
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};

    use monitor_core::Pid;

    use super::*;
    use crate::process::parse_stat;

    fn key_of(pid: u32) -> Option<ProcessKey> {
        let text = std::fs::read(format!("/proc/{pid}/stat")).ok()?;
        parse_stat(&text).map(|(_, stat)| ProcessKey {
            pid: Pid::new(pid),
            start_ticks: stat.start_ticks,
        })
    }

    #[test]
    fn pins_a_child_and_reads_it_back() {
        let Ok(mut child) = Command::new("sleep")
            .arg("30")
            .stdout(Stdio::null())
            .spawn()
        else {
            return;
        };
        let Some(key) = key_of(child.id()) else {
            let _ = child.kill();
            return;
        };
        let before = affinity(key);
        let first = before.as_ref().ok().and_then(|mask| mask.cpus().next());
        let result = first.map(|cpu| {
            let pinned = CpuMask::from_cpus([cpu]);
            (set_affinity(key, &pinned), affinity(key).ok(), pinned)
        });
        let _ = child.kill();
        let _ = child.wait();
        let Some((applied, read_back, pinned)) = result else {
            return;
        };
        assert!(applied.is_ok(), "{applied:?}");
        assert_eq!(read_back, Some(pinned));
    }

    #[test]
    fn refuses_empty_masks_stale_keys_and_the_monitor() {
        let Some(own) = key_of(std::process::id()) else {
            return;
        };
        let all = affinity(own).unwrap_or_default();
        assert!(matches!(
            set_affinity(own, &CpuMask::default()),
            Err(ActionError::NoCpus)
        ));
        let stale = ProcessKey {
            start_ticks: own.start_ticks + 1,
            ..own
        };
        assert!(matches!(set_affinity(stale, &all), Err(ActionError::Gone)));
        assert!(matches!(
            set_affinity(own, &all),
            Err(ActionError::Refused(_))
        ));
    }
}
