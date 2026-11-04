//! hwmon temperatures and fans, ACPI thermal zones and Intel throttle
//! counters, read on a 2 s cadence through persistent file handles.

mod chip;
mod discovery;
mod fans;
mod reader;
mod snapshot;
mod throttle;

use std::path::PathBuf;
use std::time::Duration;

use monitor_core::{Collector, SampleContext, SourceResult};

pub use fans::{FanControl, FanControlError};
pub use snapshot::{
    FanControlSupport, FanId, FanIdentity, FanReading, SensorCategory, SensorIdentity,
    SensorSnapshot, TemperatureReading,
};

use discovery::SensorSet;
use reader::{SlowChips, spawn_slow_chips};
use throttle::ThrottleCounters;

use crate::util::directory;
use monitor_core::util::cadence::Cadence;

const READ_INTERVAL: Duration = Duration::from_secs(2);
/// Slow chips (NVMe, EC, Wi-Fi firmware) are read off-thread this often…
const SLOW_READ_INTERVAL: Duration = Duration::from_secs(5);
/// …and this often while the window is minimized.
const MINIMIZED_SLOW_READ_INTERVAL: Duration = Duration::from_secs(15);
const DISCOVERY_INTERVAL: Duration = Duration::from_secs(10);

/// Collects [`SensorSnapshot`]s. Fast inputs are re-read at most every
/// 2 s; chips that block (NVMe, EC-backed, Wi-Fi firmware) are read every
/// 5 s (15 s minimized) on a worker thread, so `sample` never waits on
/// hardware.
#[derive(Debug)]
pub struct SensorCollector {
    sysfs: PathBuf,
    sensors: SensorSet,
    slow_chips: Option<SlowChips>,
    throttle: ThrottleCounters,
    throttle_events: Option<u64>,
    signature: u64,
    reads: Cadence,
    slow_reads: Cadence,
    discovery: Cadence,
    buffer: Vec<u8>,
}

impl Default for SensorCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl SensorCollector {
    pub fn new() -> Self {
        Self::with_sysfs(PathBuf::from("/sys"))
    }

    fn with_sysfs(sysfs: PathBuf) -> Self {
        let mut sensors = discovery::discover(&sysfs);
        Self {
            signature: hwmon_signature(&sysfs),
            slow_chips: spawn_slow_chips(std::mem::take(&mut sensors.slow_chips)),
            sensors,
            throttle: ThrottleCounters::discover(&sysfs),
            throttle_events: None,
            sysfs,
            reads: Cadence::default(),
            slow_reads: Cadence::default(),
            discovery: Cadence::default(),
            buffer: Vec::new(),
        }
    }

    fn refresh_chips(&mut self, now: Duration) {
        if !self.discovery.due(now, DISCOVERY_INTERVAL) {
            return;
        }
        let signature = hwmon_signature(&self.sysfs);
        if signature != self.signature {
            self.signature = signature;
            self.sensors = discovery::discover(&self.sysfs);
            self.slow_chips = spawn_slow_chips(std::mem::take(&mut self.sensors.slow_chips));
            self.reads.reset();
            self.slow_reads.reset();
        }
    }

    fn read_inputs(&mut self) {
        let SensorSet {
            chips,
            temperatures,
            fans,
            ..
        } = &mut self.sensors;
        for chip in chips.iter() {
            chip.read(&mut self.buffer, |reading| {
                reading.apply(temperatures, fans)
            });
        }
        self.throttle_events = self.throttle.read();
    }

    /// Takes what the slow-chip thread read since the last tick, then asks
    /// for the next read when due.
    fn exchange_slow_inputs(&mut self, now: Duration, interval: Duration) {
        let Some(slow_chips) = &self.slow_chips else {
            return;
        };
        let SensorSet {
            temperatures, fans, ..
        } = &mut self.sensors;
        slow_chips.take_fresh(|readings| {
            for reading in readings {
                reading.apply(temperatures, fans);
            }
        });
        if self.slow_reads.due(now, interval) {
            slow_chips.request();
        }
    }

    fn publish(&self, output: &mut SensorSnapshot) {
        let temperatures = &self.sensors.temperatures;
        output.temperatures.clone_from(temperatures);
        output.fans.clone_from(&self.sensors.fans);
        output.cpu_temperature = self
            .sensors
            .cpu_candidates
            .iter()
            .find_map(|&(_, index)| temperatures[index].value);
        output.hottest = temperatures
            .iter()
            .enumerate()
            .filter_map(|(index, reading)| Some((index, reading.value?)))
            .max_by(|(_, left), (_, right)| left.get().total_cmp(&right.get()))
            .map(|(index, _)| index);
        output.throttle_events = self.throttle_events;
    }
}

fn hwmon_signature(sysfs: &std::path::Path) -> u64 {
    directory::signature(&discovery::hwmon_root(sysfs), |name| {
        directory::is_numbered(name, "hwmon")
    })
}

impl Collector for SensorCollector {
    type Output = SensorSnapshot;

    fn sample(
        &mut self,
        context: &SampleContext<'_>,
        output: &mut SensorSnapshot,
    ) -> SourceResult<()> {
        let now = context.tick.monotonic();
        self.refresh_chips(now);
        if self.reads.due(now, READ_INTERVAL) {
            self.read_inputs();
        }
        let slow_interval = if context.interest.is_visible() {
            SLOW_READ_INTERVAL
        } else {
            MINIMIZED_SLOW_READ_INTERVAL
        };
        self.exchange_slow_inputs(now, slow_interval);
        self.publish(output);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_core::{Elapsed, FdIndex, Interest, Tick};

    #[test]
    fn live_reads_machine_sensors() {
        let mut collector = SensorCollector::new();
        let interest = Interest::default();
        let fds = FdIndex::default();
        let context = SampleContext {
            tick: Tick::now(),
            elapsed: Elapsed::default(),
            interest: &interest,
            fds: &fds,
        };
        let mut snapshot = SensorSnapshot::default();
        assert!(collector.sample(&context, &mut snapshot).is_ok());
        for reading in &snapshot.temperatures {
            let identity = &reading.identity;
            println!(
                "{:?} {} / {} = {:?} (high {:?}, crit {:?}) key={}",
                identity.category,
                identity.chip,
                identity.label,
                reading.value,
                identity.high,
                identity.critical,
                identity.key
            );
        }
        for fan in &snapshot.fans {
            println!(
                "fan {} / {} = {:?} rpm, max {:?}, {:?}",
                fan.identity.chip,
                fan.identity.label,
                fan.rpm,
                fan.identity.max,
                fan.identity.control
            );
        }
        println!(
            "cpu {:?}, hottest {:?}, throttle {:?}",
            snapshot.cpu_temperature,
            snapshot
                .hottest_sensor()
                .map(|reading| (&reading.identity.label, reading.value)),
            snapshot.throttle_events
        );
        let has_chip = |chip: &str| {
            snapshot
                .temperatures
                .iter()
                .any(|reading| &*reading.identity.chip == chip)
        };
        if has_chip("k10temp") {
            let cpu = snapshot.cpu_temperature.map(|celsius| celsius.get());
            assert!(
                cpu.is_some_and(|celsius| (20.0..105.0).contains(&celsius)),
                "cpu {cpu:?}"
            );
        }
        let legion_fans = snapshot
            .fans
            .iter()
            .filter(|fan| &*fan.identity.chip == "legion_hwmon")
            .count();
        if has_chip("legion_hwmon") {
            assert_eq!(legion_fans, 2);
        }
        let slow_chips = collector.slow_chips.take();
        let again = snapshot.clone();
        assert!(collector.sample(&context, &mut snapshot).is_ok());
        assert_eq!(
            again, snapshot,
            "a tick within 2 s republishes the last readings"
        );
        if let Some(slow_chips) = slow_chips {
            slow_chips.request();
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let mut delivered = false;
            while !delivered && std::time::Instant::now() < deadline {
                delivered = slow_chips.take_fresh(|readings| assert!(!readings.is_empty()));
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(delivered, "the slow-chip thread answers a request");
        }
    }
}
