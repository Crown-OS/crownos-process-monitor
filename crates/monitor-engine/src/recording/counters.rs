use std::sync::Arc;

use monitor_core::util::parse;
use monitor_core::util::procfs::ProcRoot;
use monitor_core::util::sysfs::PersistentFile;
use monitor_history::KernelCounters;

use crate::Snapshot;

const PROC_STAT: &str = "/proc/stat";
/// `user nice system idle iowait irq softirq steal`; guest time is already
/// part of `user` and `nice`.
const IDLE_FIELDS: [usize; 2] = [3, 4];
const COUNTED_FIELDS: usize = 8;

/// Since-boot totals for session gaps (plan §6.4).
pub(crate) struct KernelCounterReader {
    boot_id: Arc<str>,
    stat: Option<PersistentFile>,
    buffer: Vec<u8>,
    ticks_per_second: f64,
}

impl KernelCounterReader {
    pub fn open() -> Self {
        Self {
            boot_id: ProcRoot::open()
                .and_then(|root| root.boot_id())
                .unwrap_or_default(),
            stat: PersistentFile::open(PROC_STAT).ok(),
            buffer: Vec::with_capacity(8192),
            ticks_per_second: rustix::param::clock_ticks_per_second().max(1) as f64,
        }
    }

    pub fn read(&mut self, snapshot: &Snapshot) -> KernelCounters {
        KernelCounters {
            taken_unix_seconds: snapshot
                .tick
                .map_or(0, |tick| tick.wall_unix_seconds() as i64),
            boot_id: Arc::clone(&self.boot_id),
            cpu_busy_seconds_total: self.cpu_busy_seconds().unwrap_or(0.0),
            disk_read_total: snapshot.disk.read_total,
            disk_written_total: snapshot.disk.written_total,
            network_received_total: snapshot.network.receive_total,
            network_transmitted_total: snapshot.network.transmit_total,
        }
    }

    /// Busy time since boot in machine seconds: summed over CPUs, divided
    /// by their count, like `SystemSample::cpu_busy_seconds`.
    fn cpu_busy_seconds(&mut self) -> Option<f64> {
        let text = self.stat.as_ref()?.read(&mut self.buffer).ok()?;
        busy_machine_ticks(text).map(|ticks| ticks / self.ticks_per_second)
    }
}

fn busy_machine_ticks(stat: &[u8]) -> Option<f64> {
    let mut lines = parse::lines(stat);
    let total = lines.next()?.strip_prefix(b"cpu ")?;
    let cpus = lines
        .take_while(|line| line.starts_with(b"cpu"))
        .count()
        .max(1);
    let busy: u64 = parse::fields(total)
        .take(COUNTED_FIELDS)
        .enumerate()
        .filter(|(index, _)| !IDLE_FIELDS.contains(index))
        .filter_map(|(_, field)| parse::parse_u64(field))
        .sum();
    Some(busy as f64 / cpus as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT: &[u8] = b"cpu  1000 50 400 9000 100 20 30 0 0 0\n\
cpu0 500 25 200 4500 50 10 15 0 0 0\n\
cpu1 500 25 200 4500 50 10 15 0 0 0\n\
intr 12345 0 0\n";

    #[test]
    fn sums_busy_ticks_per_cpu() {
        assert_eq!(busy_machine_ticks(STAT), Some(750.0));
        assert_eq!(busy_machine_ticks(b"intr 1\n"), None);
    }
}
