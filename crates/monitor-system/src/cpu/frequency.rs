use monitor_core::Megahertz;
use monitor_core::util::sysfs::PersistentFile;

use super::CPU_SYSFS;
use super::snapshot::{FrequencySummary, LogicalCpuUsage};

/// Preferred: an APERF/MPERF average over the last tick on new kernels.
const AVERAGE_FREQUENCY: &str = "cpufreq/cpuinfo_avg_freq";
const CURRENT_FREQUENCY: &str = "cpufreq/scaling_cur_freq";

/// One open frequency file per online CPU, reopened when the CPU set changes.
#[derive(Debug, Default)]
pub(crate) struct FrequencyReaders {
    files: Vec<(u32, Option<PersistentFile>)>,
}

impl FrequencyReaders {
    fn matches(&self, cpus: &[LogicalCpuUsage]) -> bool {
        self.files.len() == cpus.len()
            && self
                .files
                .iter()
                .zip(cpus)
                .all(|((id, _), cpu)| *id == cpu.cpu)
    }

    fn reopen(&mut self, cpus: &[LogicalCpuUsage]) {
        self.files.clear();
        self.files.extend(cpus.iter().map(|cpu| {
            let open =
                |name| PersistentFile::open(format!("{CPU_SYSFS}/cpu{}/{name}", cpu.cpu)).ok();
            (
                cpu.cpu,
                open(AVERAGE_FREQUENCY).or_else(|| open(CURRENT_FREQUENCY)),
            )
        }));
    }

    /// Fills each CPU's frequency and returns the summary across them.
    pub fn read(&mut self, cpus: &mut [LogicalCpuUsage]) -> FrequencySummary {
        if !self.matches(cpus) {
            self.reopen(cpus);
        }
        for (cpu, (_, file)) in cpus.iter_mut().zip(&self.files) {
            cpu.frequency = file
                .as_ref()
                .and_then(|file| file.read_u64().ok())
                .map(Megahertz::from_khz);
        }
        summarize(cpus)
    }
}

fn summarize(cpus: &[LogicalCpuUsage]) -> FrequencySummary {
    let readings = || {
        cpus.iter()
            .filter_map(|cpu| cpu.frequency)
            .map(Megahertz::get)
    };
    let (sum, count) = readings().fold((0_u64, 0_u64), |(sum, count), mhz| {
        (sum + u64::from(mhz), count + 1)
    });
    FrequencySummary {
        average: (count > 0)
            .then(|| Megahertz::new(u32::try_from(sum / count.max(1)).unwrap_or(u32::MAX))),
        min: readings().min().map(Megahertz::new),
        max: readings().max().map(Megahertz::new),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_known_readings() {
        let cpus = [
            LogicalCpuUsage {
                cpu: 0,
                frequency: Some(Megahertz::new(4000)),
                ..LogicalCpuUsage::default()
            },
            LogicalCpuUsage {
                cpu: 1,
                frequency: Some(Megahertz::new(2000)),
                ..LogicalCpuUsage::default()
            },
            LogicalCpuUsage::default(),
        ];
        let summary = summarize(&cpus);
        assert_eq!(summary.average, Some(Megahertz::new(3000)));
        assert_eq!(summary.min, Some(Megahertz::new(2000)));
        assert_eq!(summary.max, Some(Megahertz::new(4000)));
        assert_eq!(summarize(&[]).average, None);
    }
}
