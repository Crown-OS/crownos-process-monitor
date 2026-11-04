use std::collections::HashMap;
use std::time::{Duration, Instant};

use monitor_core::util::procfs::ProcRoot;
use monitor_core::util::sysfs;
use monitor_core::{Pid, ProcessKey};
use monitor_system::{IoCounters, SmapsRollup, parse_stat};

use crate::{HelperError, ProcessUsage};

/// Bounds one request; a machine has far fewer processes.
pub(crate) const MAX_REQUESTED_PIDS: usize = 1 << 16;
/// `smaps_rollup` makes the kernel walk page tables, so PSS is measured for
/// at most this many processes per call, round-robin…
const PSS_PER_CALL: usize = 64;
/// …and for no longer than this.
const PSS_TIME_BUDGET: Duration = Duration::from_millis(4);
/// Measurements of processes no request mentioned for this many calls are
/// dropped.
const STALE_AFTER_CALLS: u64 = 120;

struct MeasuredRollup {
    rollup: SmapsRollup,
    last_requested: u64,
}

/// Disk counters and memory rollups of any user's processes.
pub(crate) struct ProcessUsageReader {
    root: ProcRoot,
    pids: Vec<Pid>,
    kernel_threads: Vec<bool>,
    buffer: Vec<u8>,
    rollups: HashMap<ProcessKey, MeasuredRollup>,
    last_measured_pid: u32,
    call: u64,
}

impl ProcessUsageReader {
    pub(crate) fn open() -> Result<Self, HelperError> {
        Ok(Self {
            root: ProcRoot::open().map_err(HelperError::failed)?,
            pids: Vec::new(),
            kernel_threads: Vec::new(),
            buffer: Vec::new(),
            rollups: HashMap::new(),
            last_measured_pid: 0,
            call: 0,
        })
    }

    /// Figures for `requested`, or for every process when it is empty.
    /// Processes that exited are left out.
    pub(crate) fn sample(&mut self, requested: &[u32]) -> Result<Vec<ProcessUsage>, HelperError> {
        if requested.len() > MAX_REQUESTED_PIDS {
            return Err(HelperError::invalid(format!(
                "at most {MAX_REQUESTED_PIDS} pids per request"
            )));
        }
        self.call += 1;
        self.select(requested)?;
        let mut usages = Vec::with_capacity(self.pids.len());
        self.kernel_threads.clear();
        for index in 0..self.pids.len() {
            if let Some((usage, kernel_thread)) = self.read_process(self.pids[index]) {
                usages.push(usage);
                self.kernel_threads.push(kernel_thread);
            }
        }
        self.measure_rollups(&mut usages);
        let call = self.call;
        self.rollups
            .retain(|_, measured| call - measured.last_requested < STALE_AFTER_CALLS);
        Ok(usages)
    }

    fn select(&mut self, requested: &[u32]) -> Result<(), HelperError> {
        if requested.is_empty() {
            return self
                .root
                .list_pids(&mut self.pids)
                .map_err(HelperError::failed);
        }
        self.pids.clear();
        self.pids.extend(
            requested
                .iter()
                .copied()
                .filter(|&pid| pid > 0)
                .map(Pid::new),
        );
        self.pids.sort_unstable();
        self.pids.dedup();
        Ok(())
    }

    fn read_process(&mut self, pid: Pid) -> Option<(ProcessUsage, bool)> {
        let directory = self.root.open_pid(pid).ok()?;
        let (_, stat) = parse_stat(sysfs::read_at(&directory, "stat", &mut self.buffer).ok()?)?;
        let disk = sysfs::read_at(&directory, "io", &mut self.buffer)
            .map(IoCounters::parse)
            .unwrap_or_default();
        let key = ProcessKey {
            pid,
            start_ticks: stat.start_ticks,
        };
        let rollup = self
            .rollups
            .get_mut(&key)
            .map(|measured| {
                measured.last_requested = self.call;
                measured.rollup
            })
            .unwrap_or_default();
        let usage = ProcessUsage {
            pid: pid.get(),
            start_ticks: stat.start_ticks,
            read_bytes: disk.read_bytes,
            written_bytes: disk.write_bytes,
            cancelled_written_bytes: disk.cancelled_write_bytes,
            ..ProcessUsage::default()
        };
        Some((with_rollup(usage, rollup), stat.is_kernel_thread()))
    }

    /// Continues the round-robin after the PID measured last.
    fn measure_rollups(&mut self, usages: &mut [ProcessUsage]) {
        let deadline = Instant::now() + PSS_TIME_BUDGET;
        let resume_at = usages.partition_point(|usage| usage.pid <= self.last_measured_pid);
        let mut measured = 0;
        for index in (resume_at..usages.len()).chain(0..resume_at) {
            if measured == PSS_PER_CALL || Instant::now() >= deadline {
                break;
            }
            if self.kernel_threads[index] {
                continue;
            }
            let usage = &mut usages[index];
            self.last_measured_pid = usage.pid;
            let Some(rollup) = self.read_rollup(Pid::new(usage.pid)) else {
                continue;
            };
            measured += 1;
            *usage = with_rollup(*usage, rollup);
            self.rollups.insert(
                ProcessKey {
                    pid: Pid::new(usage.pid),
                    start_ticks: usage.start_ticks,
                },
                MeasuredRollup {
                    rollup,
                    last_requested: self.call,
                },
            );
        }
    }

    fn read_rollup(&mut self, pid: Pid) -> Option<SmapsRollup> {
        let directory = self.root.open_pid(pid).ok()?;
        sysfs::read_at(&directory, "smaps_rollup", &mut self.buffer)
            .ok()
            .map(SmapsRollup::parse)
    }
}

fn with_rollup(usage: ProcessUsage, rollup: SmapsRollup) -> ProcessUsage {
    ProcessUsage {
        pss: rollup.proportional.get(),
        uss: rollup.unique.get(),
        swap: rollup.swap.get(),
        ..usage
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_own_process() {
        let Ok(mut reader) = ProcessUsageReader::open() else {
            return;
        };
        let own = std::process::id();
        let usages = reader.sample(&[own, own, 0]);
        let Ok(usages) = usages else {
            panic!("sampling failed: {usages:?}");
        };
        assert_eq!(usages.len(), 1, "duplicates and pid 0 are dropped");
        assert_eq!(usages[0].pid, own);
        assert!(usages[0].start_ticks > 0);
        assert!(usages[0].pss > 0, "a lone process is measured at once");
        assert!(usages[0].uss > 0);
        let again = reader.sample(&[own]).unwrap_or_default();
        assert_eq!(
            again.first().map(|usage| usage.start_ticks),
            Some(usages[0].start_ticks)
        );
    }

    #[test]
    fn lists_every_process_when_none_requested() {
        let Ok(mut reader) = ProcessUsageReader::open() else {
            return;
        };
        let usages = reader.sample(&[]).unwrap_or_default();
        assert!(usages.iter().any(|usage| usage.pid == std::process::id()));
        assert!(usages.windows(2).all(|pair| pair[0].pid < pair[1].pid));
        assert!(matches!(
            reader.sample(&vec![1; MAX_REQUESTED_PIDS + 1]),
            Err(HelperError::InvalidArgument(_))
        ));
    }

    #[test]
    fn skips_vanished_processes() {
        let Ok(mut reader) = ProcessUsageReader::open() else {
            return;
        };
        let usages = reader.sample(&[u32::MAX - 1]).unwrap_or_default();
        assert!(usages.is_empty());
    }
}
