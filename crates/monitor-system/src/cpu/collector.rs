use std::time::Duration;

use monitor_core::util::cadence::Cadence;
use monitor_core::util::parse::{fields, parse_f64, parse_u32};
use monitor_core::util::sysfs::PersistentFile;
use monitor_core::{Collector, Page, Percent, SampleContext, SourceResult};

use super::frequency::FrequencyReaders;
use super::policy::PolicyReader;
use super::snapshot::{
    CpuSnapshot, CpuStates, EventRates, FrequencySummary, LoadAverage, LogicalCpuUsage,
};
use super::stat::{CpuTimes, KernelCounters, parse_stat};
use crate::util::{clock, psi};

const FREQUENCY_INTERVAL: Duration = Duration::from_secs(2);
const POLICY_INTERVAL: Duration = Duration::from_secs(10);

/// `/proc/stat`, `/proc/loadavg`, `/proc/pressure/cpu` and cpufreq, read
/// through descriptors kept open between samples.
pub struct CpuCollector {
    stat: PersistentFile,
    loadavg: PersistentFile,
    pressure: Option<PersistentFile>,
    buffer: Vec<u8>,
    total: CpuTimes,
    previous_total: Option<CpuTimes>,
    per_cpu: Vec<(u32, CpuTimes)>,
    previous_per_cpu: Vec<(u32, CpuTimes)>,
    counters: KernelCounters,
    previous_counters: Option<KernelCounters>,
    frequency: FrequencyReaders,
    frequency_reads: Cadence,
    policy: PolicyReader,
    policy_reads: Cadence,
}

impl CpuCollector {
    pub fn new() -> SourceResult<Self> {
        Ok(Self {
            stat: PersistentFile::open("/proc/stat")?,
            loadavg: PersistentFile::open("/proc/loadavg")?,
            pressure: PersistentFile::open("/proc/pressure/cpu").ok(),
            buffer: Vec::with_capacity(8192),
            total: CpuTimes::default(),
            previous_total: None,
            per_cpu: Vec::new(),
            previous_per_cpu: Vec::new(),
            counters: KernelCounters::default(),
            previous_counters: None,
            frequency: FrequencyReaders::default(),
            frequency_reads: Cadence::default(),
            policy: PolicyReader::open(),
            policy_reads: Cadence::default(),
        })
    }

    fn read_stat(&mut self) -> SourceResult<()> {
        std::mem::swap(&mut self.per_cpu, &mut self.previous_per_cpu);
        self.previous_total = Some(self.total).filter(|times| times.total() > 0);
        self.previous_counters = Some(self.counters).filter(|counters| counters.boot_time > 0);
        let bytes = self.stat.read(&mut self.buffer)?;
        parse_stat(
            bytes,
            &mut self.total,
            &mut self.per_cpu,
            &mut self.counters,
        );
        Ok(())
    }

    fn fill_usage(&self, output: &mut CpuSnapshot, rates_valid: bool) {
        let previous_total = self.previous_total.filter(|_| rates_valid);
        let (total, states) =
            previous_total.map_or_else(Default::default, |previous| shares(&previous, &self.total));
        output.total = total;
        output.states = states;

        if output.logical.len() != self.per_cpu.len() {
            output
                .logical
                .resize(self.per_cpu.len(), LogicalCpuUsage::default());
        }
        let same_layout = self.previous_per_cpu.len() == self.per_cpu.len();
        for (index, (cpu, (id, times))) in output.logical.iter_mut().zip(&self.per_cpu).enumerate()
        {
            let previous = self
                .previous_per_cpu
                .get(index)
                .filter(|(previous_id, _)| same_layout && rates_valid && previous_id == id);
            if cpu.cpu != *id {
                cpu.frequency = None;
            }
            cpu.cpu = *id;
            cpu.usage = previous.map_or(Percent::ZERO, |(_, previous)| shares(previous, times).0);
        }
    }

    fn fill_events(&self, output: &mut CpuSnapshot, seconds: f64) {
        let current = &self.counters;
        output.procs_running = current.procs_running;
        output.procs_blocked = current.procs_blocked;
        output.events = self
            .previous_counters
            .filter(|_| seconds > 0.0)
            .map_or_else(EventRates::default, |previous| {
                let rate = |earlier: u64, later: u64| {
                    (later.saturating_sub(earlier) as f64 / seconds) as f32
                };
                EventRates {
                    context_switches: rate(previous.context_switches, current.context_switches),
                    interrupts: rate(previous.interrupts, current.interrupts),
                    soft_interrupts: rate(previous.soft_interrupts, current.soft_interrupts),
                    forks: rate(previous.forks, current.forks),
                }
            });
    }

    fn read_load(&mut self, output: &mut CpuSnapshot) -> SourceResult<()> {
        let bytes = self.loadavg.read(&mut self.buffer)?;
        let mut columns = fields(bytes);
        let mut average = || columns.next().and_then(parse_f64).unwrap_or(0.0) as f32;
        output.load = LoadAverage {
            one: average(),
            five: average(),
            fifteen: average(),
        };
        output.threads = fields(bytes)
            .nth(3)
            .and_then(|entities| entities.split(|&byte| byte == b'/').nth(1))
            .and_then(parse_u32)
            .unwrap_or(0);
        Ok(())
    }

    fn read_pressure(&mut self, output: &mut CpuSnapshot) {
        output.pressure = self
            .pressure
            .as_ref()
            .and_then(|file| file.read(&mut self.buffer).ok())
            .and_then(|bytes| psi::parse_pressure(bytes).0);
    }
}

impl Collector for CpuCollector {
    type Output = CpuSnapshot;

    fn sample(
        &mut self,
        context: &SampleContext<'_>,
        output: &mut CpuSnapshot,
    ) -> SourceResult<()> {
        self.read_stat()?;
        let rates_valid = !context.elapsed.suspended && context.elapsed.seconds > 0.0;
        output.gap = !rates_valid || self.previous_total.is_none();
        self.fill_usage(output, rates_valid);
        self.fill_events(
            output,
            if rates_valid {
                context.elapsed.seconds
            } else {
                0.0
            },
        );
        self.read_load(output)?;
        self.read_pressure(output);
        output.uptime = clock::since_boot();

        let now = context.tick.monotonic();
        if context.interest.shows(Page::Cpu) {
            if self.frequency_reads.due(now, FREQUENCY_INTERVAL) {
                output.frequency = self.frequency.read(&mut output.logical);
            }
        } else if self.frequency_reads.reset() {
            output.frequency = FrequencySummary::default();
            output
                .logical
                .iter_mut()
                .for_each(|cpu| cpu.frequency = None);
        }
        if self.policy_reads.due(now, POLICY_INTERVAL) {
            self.policy.refresh(&mut output.policy);
        }
        Ok(())
    }
}

/// Busy share per §2.2 and the per-state shares between two readings.
fn shares(previous: &CpuTimes, current: &CpuTimes) -> (Percent, CpuStates) {
    let delta = |earlier: u64, later: u64| later.saturating_sub(earlier);
    let elapsed = delta(previous.total(), current.total());
    if elapsed == 0 {
        return (Percent::ZERO, CpuStates::default());
    }
    let share = |ticks: u64| Percent::clamped((ticks as f64 * 100.0 / elapsed as f64) as f32);
    let guest = delta(previous.guest, current.guest);
    let guest_nice = delta(previous.guest_nice, current.guest_nice);
    let states = CpuStates {
        user: share(delta(previous.user, current.user).saturating_sub(guest)),
        nice: share(delta(previous.nice, current.nice).saturating_sub(guest_nice)),
        system: share(delta(previous.system, current.system)),
        idle: share(delta(previous.idle, current.idle)),
        iowait: share(delta(previous.iowait, current.iowait)),
        irq: share(delta(previous.irq, current.irq)),
        softirq: share(delta(previous.softirq, current.softirq)),
        steal: share(delta(previous.steal, current.steal)),
        guest: share(guest + guest_nice),
    };
    (share(delta(previous.busy(), current.busy())), states)
}

#[cfg(test)]
mod tests {
    use monitor_core::{Elapsed, FdIndex, Interest, Tick};

    use super::*;

    #[test]
    fn shares_exclude_guest_from_user() {
        let previous = CpuTimes::default();
        let current = CpuTimes {
            user: 50,
            system: 10,
            idle: 30,
            iowait: 10,
            guest: 20,
            ..CpuTimes::default()
        };
        let (total, states) = shares(&previous, &current);
        assert_eq!(total.get(), 60.0);
        assert_eq!(states.user.get(), 30.0);
        assert_eq!(states.guest.get(), 20.0);
        assert_eq!(states.iowait.get(), 10.0);
        let stacked = states.user.get()
            + states.guest.get()
            + states.system.get()
            + states.idle.get()
            + states.iowait.get();
        assert_eq!(stacked, 100.0);
    }

    #[test]
    fn samples_live_machine() -> SourceResult<()> {
        let mut collector = CpuCollector::new()?;
        let interest = Interest {
            page: Page::Cpu,
            ..Interest::default()
        };
        let fds = FdIndex::default();
        let mut snapshot = CpuSnapshot::default();
        let first = Tick::now();
        let context = SampleContext {
            tick: first,
            elapsed: Elapsed::default(),
            interest: &interest,
            fds: &fds,
        };
        collector.sample(&context, &mut snapshot)?;
        assert!(snapshot.gap);
        std::thread::sleep(Duration::from_millis(200));
        let second = Tick::now();
        let context = SampleContext {
            tick: second,
            elapsed: second.since(&first),
            interest: &interest,
            fds: &fds,
        };
        collector.sample(&context, &mut snapshot)?;
        assert!(!snapshot.gap);
        assert!((0.0..=100.0).contains(&snapshot.total.get()));
        assert!(!snapshot.logical.is_empty());
        assert!(snapshot.logical.iter().all(|cpu| cpu.usage.get() <= 100.0));
        assert!(snapshot.events.context_switches > 0.0);
        assert!(snapshot.threads > 0);
        assert!(snapshot.uptime > Duration::ZERO);
        Ok(())
    }

    #[test]
    fn suspended_interval_has_no_rates() -> SourceResult<()> {
        let mut collector = CpuCollector::new()?;
        let interest = Interest::default();
        let fds = FdIndex::default();
        let mut snapshot = CpuSnapshot::default();
        let tick = Tick::now();
        let mut context = SampleContext {
            tick,
            elapsed: Elapsed::default(),
            interest: &interest,
            fds: &fds,
        };
        collector.sample(&context, &mut snapshot)?;
        context.elapsed = Elapsed {
            seconds: 1.0,
            suspended: true,
        };
        collector.sample(&context, &mut snapshot)?;
        assert!(snapshot.gap);
        assert_eq!(snapshot.events, EventRates::default());
        assert_eq!(snapshot.total, Percent::ZERO);
        Ok(())
    }
}
