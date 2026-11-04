use monitor_core::util::parse::{fields, lines, parse_u32, parse_u64};

/// Cumulative jiffies of one `cpu` line of `/proc/stat`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CpuTimes {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
    pub steal: u64,
    pub guest: u64,
    pub guest_nice: u64,
}

impl CpuTimes {
    fn parse<'a>(columns: impl Iterator<Item = &'a [u8]>) -> Self {
        let mut values = [0_u64; 10];
        for (slot, column) in values.iter_mut().zip(columns) {
            *slot = parse_u64(column).unwrap_or(0);
        }
        let [
            user,
            nice,
            system,
            idle,
            iowait,
            irq,
            softirq,
            steal,
            guest,
            guest_nice,
        ] = values;
        Self {
            user,
            nice,
            system,
            idle,
            iowait,
            irq,
            softirq,
            steal,
            guest,
            guest_nice,
        }
    }

    /// Guest time is already part of `user` and `nice`.
    pub fn busy(&self) -> u64 {
        self.user + self.nice + self.system + self.irq + self.softirq + self.steal
    }

    pub fn total(&self) -> u64 {
        self.busy() + self.idle + self.iowait
    }
}

/// The system-wide counters at the end of `/proc/stat`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct KernelCounters {
    pub context_switches: u64,
    pub interrupts: u64,
    pub soft_interrupts: u64,
    pub forks: u64,
    pub procs_running: u32,
    pub procs_blocked: u32,
    pub boot_time: u64,
}

/// Parses `/proc/stat` into the aggregate line, the per-CPU lines (as
/// `(cpu id, times)`) and the counters, reusing `per_cpu`.
pub(crate) fn parse_stat(
    buffer: &[u8],
    total: &mut CpuTimes,
    per_cpu: &mut Vec<(u32, CpuTimes)>,
    counters: &mut KernelCounters,
) {
    per_cpu.clear();
    for line in lines(buffer) {
        let mut columns = fields(line);
        let Some(label) = columns.next() else {
            continue;
        };
        let first = || fields(line).nth(1).and_then(parse_u64).unwrap_or(0);
        match label {
            b"cpu" => *total = CpuTimes::parse(columns),
            b"intr" => counters.interrupts = first(),
            b"ctxt" => counters.context_switches = first(),
            b"btime" => counters.boot_time = first(),
            b"processes" => counters.forks = first(),
            b"softirq" => counters.soft_interrupts = first(),
            b"procs_running" => {
                counters.procs_running = parse_u32(columns.next().unwrap_or(b"")).unwrap_or(0)
            }
            b"procs_blocked" => {
                counters.procs_blocked = parse_u32(columns.next().unwrap_or(b"")).unwrap_or(0)
            }
            _ => {
                if let Some(id) = label.strip_prefix(b"cpu").and_then(parse_u32) {
                    per_cpu.push((id, CpuTimes::parse(columns)));
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const STAT: &[u8] = b"cpu  119920 152 29367 1792290 7914 6770 2556 0 0 0\n\
cpu0 5711 9 1782 113689 461 343 851 0 0 0\n\
cpu1 3766 0 769 117051 157 127 192 0 0 0\n\
cpu2 13768 20 3213 102668 1189 1975 148 0 0 0\n\
cpu3 10313 4 1826 109140 682 262 89 0 0 0\n\
intr 20047169 134 6 0 0 0 0 0 42583 0 124 1429744 0 0\n\
ctxt 17518824\n\
btime 1793799635\n\
processes 45568\n\
procs_running 1\n\
procs_blocked 0\n\
softirq 4504454 5703 412025 11 79032 119 0 487959 1702931 1625 1815049\n";

    #[test]
    fn parses_real_stat() {
        let mut total = CpuTimes::default();
        let mut per_cpu = Vec::new();
        let mut counters = KernelCounters::default();
        parse_stat(STAT, &mut total, &mut per_cpu, &mut counters);
        assert_eq!(total.user, 119_920);
        assert_eq!(total.iowait, 7914);
        assert_eq!(total.busy(), 119_920 + 152 + 29_367 + 6770 + 2556);
        assert_eq!(per_cpu.len(), 4);
        assert_eq!(per_cpu[2].0, 2);
        assert_eq!(per_cpu[2].1.user, 13_768);
        assert_eq!(counters.interrupts, 20_047_169);
        assert_eq!(counters.context_switches, 17_518_824);
        assert_eq!(counters.forks, 45_568);
        assert_eq!(counters.soft_interrupts, 4_504_454);
        assert_eq!(counters.procs_running, 1);
        assert_eq!(counters.boot_time, 1_793_799_635);
    }
}
