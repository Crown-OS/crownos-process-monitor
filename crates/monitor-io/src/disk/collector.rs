use std::sync::Arc;
use std::time::Duration;

use monitor_core::util::deferred::DeferredReader;
use monitor_core::util::parse;
use monitor_core::util::sysfs::PersistentFile;
use monitor_core::{Celsius, Collector, Percent, SampleContext, SourceResult};

use super::device::{self, DeviceInfo};
use super::diskstats::{self, DiskCounters, DiskRates};
use super::mount_watch::MountWatch;
use super::pressure;
use super::temperature::DriveSensor;
use super::udisks::{HealthTable, UDisks};
use super::volumes::{self, Volume};
use super::{DiskDevice, DiskSnapshot};
use crate::RateStatus;
use crate::util::fingerprint::Fingerprint;
use monitor_core::util::cadence::interval_elapsed;
use monitor_core::util::latest::Latest;

const DISKSTATS: &str = "/proc/diskstats";
const IO_PRESSURE: &str = "/proc/pressure/io";
const CAPACITY_INTERVAL: Duration = Duration::from_secs(10);
const TEMPERATURE_INTERVAL: Duration = Duration::from_secs(10);
const HEALTH_INTERVAL: Duration = Duration::from_secs(5 * 60);
const TEMPERATURE_THREAD: &str = "monitor-drive-temperatures";

/// Drive temperatures in disk order. NVMe drives answer each read with an
/// admin command (milliseconds), so a worker thread reads them.
type TemperatureReader = DeferredReader<Vec<Option<Celsius>>>;

#[derive(Debug)]
struct TrackedDisk {
    info: Arc<DeviceInfo>,
    temperature: Option<Celsius>,
    previous: Option<DiskCounters>,
    current: Option<DiskCounters>,
}

/// Samples `/proc/diskstats`, PSI, volumes and drive temperatures on the
/// sampler thread; SMART comes from the UDisks2 worker it starts.
#[derive(Debug)]
pub struct DiskCollector {
    diskstats: PersistentFile,
    pressure: Option<PersistentFile>,
    mounts: Option<MountWatch>,
    buffer: Vec<u8>,
    disks: Vec<TrackedDisk>,
    fingerprint: Option<Fingerprint>,
    previous_total: Option<DiskCounters>,
    volumes: Vec<Volume>,
    capacity_refreshed: Option<Duration>,
    temperature_read: Option<Duration>,
    temperatures: Option<TemperatureReader>,
    health_requested: Option<Duration>,
    health: Arc<Latest<HealthTable>>,
    health_seen: Arc<HealthTable>,
    udisks: UDisks,
}

impl DiskCollector {
    /// Opens the hot files and starts the UDisks2 worker thread.
    pub fn new() -> SourceResult<Self> {
        let health = Arc::new(Latest::default());
        Ok(Self {
            diskstats: PersistentFile::open(DISKSTATS)?,
            pressure: PersistentFile::open(IO_PRESSURE).ok(),
            mounts: MountWatch::open().ok(),
            buffer: Vec::new(),
            disks: Vec::new(),
            fingerprint: None,
            previous_total: None,
            volumes: Vec::new(),
            capacity_refreshed: None,
            temperature_read: None,
            temperatures: None,
            health_requested: None,
            udisks: UDisks::spawn(Arc::clone(&health)),
            health,
            health_seen: Arc::new(HealthTable::default()),
        })
    }

    /// Handle for unmount / eject / power-off requests and SMART refreshes.
    pub fn udisks(&self) -> UDisks {
        self.udisks.clone()
    }

    fn read_counters(&mut self) -> SourceResult<()> {
        let mut buffer = std::mem::take(&mut self.buffer);
        let outcome = self.track_counters(&mut buffer);
        self.buffer = buffer;
        outcome
    }

    fn track_counters(&mut self, buffer: &mut Vec<u8>) -> SourceResult<()> {
        let text = self.diskstats.read(buffer)?;
        let mut fingerprint = Fingerprint::default();
        for line in parse::lines(text) {
            if let Some((name, _)) = diskstats::parse_line(line) {
                fingerprint.add(name);
            }
        }
        if self.fingerprint != Some(fingerprint) {
            self.fingerprint = Some(fingerprint);
            self.rediscover();
        }
        for disk in &mut self.disks {
            disk.current = None;
        }
        for (name, counters) in parse::lines(text).filter_map(diskstats::parse_line) {
            if let Some(disk) = self
                .disks
                .iter_mut()
                .find(|disk| disk.info.key.as_str().as_bytes() == name)
            {
                disk.current = Some(counters);
            }
        }
        Ok(())
    }

    /// Hotplug: re-reads `/sys/block`, keeping counters of disks that stayed.
    fn rediscover(&mut self) {
        let previous = std::mem::take(&mut self.disks);
        self.disks = device::physical_disks()
            .into_iter()
            .map(|name| {
                let kept = previous
                    .iter()
                    .find(|disk| disk.info.key.as_str() == &*name);
                TrackedDisk {
                    info: Arc::new(device::describe(&name)),
                    temperature: kept.and_then(|disk| disk.temperature),
                    previous: kept.and_then(|disk| disk.previous),
                    current: None,
                }
            })
            .collect();
        self.previous_total = None;
        self.temperatures = spawn_temperature_reader(&self.disks);
        self.temperature_read = None;
    }

    fn refresh_temperatures(&mut self, now: Duration) {
        let Some(reader) = &self.temperatures else {
            return;
        };
        let disks = &mut self.disks;
        reader.take_fresh(|temperatures| {
            for (disk, temperature) in disks.iter_mut().zip(temperatures) {
                disk.temperature = *temperature;
            }
        });
        if interval_elapsed(self.temperature_read, now, TEMPERATURE_INTERVAL) {
            self.temperature_read = Some(now);
            reader.request();
        }
    }

    fn refresh_health(&mut self, now: Duration) {
        if interval_elapsed(self.health_requested, now, HEALTH_INTERVAL) {
            self.health_requested = Some(now);
            self.udisks.refresh_health();
        }
        if let Some(latest) = self.health.try_get() {
            self.health_seen = latest;
        }
    }

    /// Re-parses mounts after a change; `statvfs` every 10 s otherwise.
    /// Returns whether the volume list changed.
    fn refresh_volumes(&mut self, now: Duration) -> bool {
        let mounts_changed = self.capacity_refreshed.is_none()
            || self.mounts.as_ref().is_some_and(MountWatch::changed);
        if mounts_changed && let Some(mounts) = &self.mounts {
            let mut buffer = std::mem::take(&mut self.buffer);
            if let Ok(text) = mounts.read(&mut buffer) {
                self.volumes = volumes::build(text);
            }
            self.buffer = buffer;
        }
        if !mounts_changed && !interval_elapsed(self.capacity_refreshed, now, CAPACITY_INTERVAL) {
            return false;
        }
        self.capacity_refreshed = Some(now);
        volumes::refresh_capacity(&mut self.volumes);
        true
    }

    fn read_pressure(&mut self) -> Option<pressure::IoPressure> {
        let file = self.pressure.as_ref()?;
        let text = file.read(&mut self.buffer).ok()?;
        pressure::parse_pressure(text)
    }
}

impl Collector for DiskCollector {
    type Output = DiskSnapshot;

    fn sample(
        &mut self,
        context: &SampleContext<'_>,
        output: &mut DiskSnapshot,
    ) -> SourceResult<()> {
        let now = context.tick.monotonic();
        self.read_counters()?;
        self.refresh_temperatures(now);
        self.refresh_health(now);
        if self.refresh_volumes(now) {
            output.volumes = Arc::from(self.volumes.as_slice());
        }
        output.pressure = self.read_pressure();

        let seconds = context.elapsed.seconds;
        let suspended = context.elapsed.suspended;
        let mut total = DiskCounters::default();
        let mut busiest = Percent::ZERO;
        output.devices.clear();
        for disk in &mut self.disks {
            let Some(current) = disk.current else {
                continue;
            };
            total.accumulate(&current);
            let rates = match disk.previous {
                Some(previous) if !suspended => DiskRates::between(&previous, &current, seconds),
                _ => DiskRates::idle(&current),
            };
            disk.previous = Some(current);
            if rates.utilization > busiest {
                busiest = rates.utilization;
            }
            let health = self.health_seen.health(disk.info.key.as_str());
            output.devices.push(DiskDevice {
                info: Arc::clone(&disk.info),
                rates,
                read_total: current.read_bytes(),
                written_total: current.written_bytes(),
                temperature: disk.temperature.or_else(|| health.temperature()),
                health,
            });
        }
        output.status = RateStatus::from_elapsed(self.previous_total.is_some(), suspended);
        output.totals = match self.previous_total {
            Some(previous) if !suspended => DiskRates {
                utilization: busiest,
                ..DiskRates::between(&previous, &total, seconds)
            },
            _ => DiskRates::idle(&total),
        };
        output.read_total = total.read_bytes();
        output.written_total = total.written_bytes();
        self.previous_total = Some(total);
        Ok(())
    }
}

fn spawn_temperature_reader(disks: &[TrackedDisk]) -> Option<TemperatureReader> {
    let sensors: Vec<Option<DriveSensor>> = disks
        .iter()
        .map(|disk| DriveSensor::find(disk.info.key.as_str()))
        .collect();
    if sensors.iter().all(Option::is_none) {
        return None;
    }
    DeferredReader::spawn(
        TEMPERATURE_THREAD,
        sensors,
        |sensors: &mut Vec<Option<DriveSensor>>, out: &mut Vec<Option<Celsius>>| {
            out.clear();
            out.extend(
                sensors
                    .iter()
                    .map(|sensor| sensor.as_ref().and_then(DriveSensor::read)),
            );
        },
    )
}

#[cfg(test)]
mod tests {
    use monitor_core::{FdIndex, Interest, Tick};

    use super::*;

    #[test]
    #[ignore = "samples the block devices of a real machine, not a container"]
    fn samples_physical_disks_and_volumes() {
        let Ok(mut collector) = DiskCollector::new() else {
            return;
        };
        let interest = Interest::default();
        let fds = FdIndex::default();
        let mut output = DiskSnapshot::default();
        let mut earlier = Tick::now();
        for round in 0..2 {
            std::thread::sleep(Duration::from_millis(50));
            let tick = Tick::now();
            let context = SampleContext {
                tick,
                elapsed: tick.since(&earlier),
                interest: &interest,
                fds: &fds,
            };
            assert!(collector.sample(&context, &mut output).is_ok());
            earlier = tick;
            let expected = if round == 0 {
                RateStatus::Warmup
            } else {
                RateStatus::Measured
            };
            assert_eq!(output.status, expected);
        }
        assert!(
            !output
                .devices
                .iter()
                .any(|disk| disk.info.key.as_str().starts_with("zram"))
        );
        if std::path::Path::new("/sys/block/nvme0n1").exists() {
            let names: Vec<_> = output
                .devices
                .iter()
                .map(|disk| disk.info.key.as_str())
                .collect();
            assert!(
                names.contains(&"nvme0n1") && names.contains(&"nvme1n1"),
                "{names:?}"
            );
            assert!(output.devices.iter().any(|disk| disk.temperature.is_some()));
        }
        assert!(output.read_total.get() > 0);
        assert!(!output.volumes.is_empty());
        assert!(output.pressure.is_some());
    }
}
