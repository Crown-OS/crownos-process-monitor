use std::collections::HashMap;

use monitor_core::util::time::{MINUTE_SECONDS, align_down};
use monitor_core::{AppKey, Bytes, DeviceKey};

use crate::batch::{AppMinute, GpuMinute, RollupBatch, SystemMinute, add_optional};
use crate::gauge::GaugeMetric;
use crate::gpu_engine::EngineTimes;
use crate::sample::{
    AppSample, DeviceSample, GpuSample, InterfaceSample, SystemSample, TickSamples,
};

/// Wall time may disagree with monotonic time by this much (whole-second
/// wall readings plus scheduling jitter) before it counts as a clock jump.
const WALL_JUMP_TOLERANCE_SECONDS: f64 = 3.0;

/// Builds UTC-aligned minute buckets from per-tick samples. Vectors keep
/// their capacity across ticks; the only allocations happen once a minute,
/// when a finished [`RollupBatch`] is handed out.
#[derive(Debug, Default)]
pub struct Accumulator {
    minute_start: Option<i64>,
    last_wall_unix: Option<i64>,
    system: SystemMinute,
    apps: Vec<AppMinute>,
    app_slots: HashMap<AppKey, usize>,
    gpus: Vec<GpuMinute>,
    devices: Vec<DeviceSample>,
    interfaces: Vec<InterfaceSample>,
}

impl Accumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one tick. Returns the previous minute once a tick lands in a new
    /// minute, or after a wall-clock jump.
    #[must_use]
    pub fn record(&mut self, tick: &TickSamples<'_>) -> Option<RollupBatch> {
        let system = tick.system;
        let jumped = self.wall_clock_jumped(system);
        self.last_wall_unix = Some(system.wall_unix_seconds);
        if system.suspended || system.elapsed_seconds <= 0.0 {
            return None;
        }
        let minute = align_down(midpoint_unix(system), MINUTE_SECONDS);
        let finished = if jumped || self.minute_start.is_some_and(|start| start != minute) {
            self.take_batch()
        } else {
            None
        };
        if self.minute_start.is_none() {
            self.minute_start = Some(minute);
            self.system.start_unix = minute;
        }
        self.add_system(system);
        let seconds = system.elapsed_seconds;
        tick.apps
            .iter()
            .for_each(|sample| self.add_app(sample, seconds));
        tick.gpus
            .iter()
            .for_each(|sample| self.add_gpu(sample, seconds));
        tick.devices.iter().for_each(|sample| {
            let total = slot(&mut self.devices, &sample.key, || DeviceSample {
                key: sample.key.clone(),
                read: Bytes::ZERO,
                written: Bytes::ZERO,
            });
            total.read += sample.read;
            total.written += sample.written;
        });
        tick.interfaces.iter().for_each(|sample| {
            let total = slot(&mut self.interfaces, &sample.key, || InterfaceSample {
                key: sample.key.clone(),
                received: Bytes::ZERO,
                transmitted: Bytes::ZERO,
            });
            total.received += sample.received;
            total.transmitted += sample.transmitted;
        });
        finished
    }

    /// Hands out the unfinished minute, e.g. at shutdown.
    #[must_use]
    pub fn flush(&mut self) -> Option<RollupBatch> {
        self.take_batch()
    }

    /// The minute being accumulated, for "live" figures not yet persisted.
    pub fn pending(&self) -> Option<&SystemMinute> {
        self.minute_start.map(|_| &self.system)
    }

    fn wall_clock_jumped(&self, system: &SystemSample) -> bool {
        let Some(last_wall) = self.last_wall_unix else {
            return false;
        };
        let wall_delta = (system.wall_unix_seconds - last_wall) as f64;
        !system.suspended
            && (wall_delta - system.elapsed_seconds).abs() > WALL_JUMP_TOLERANCE_SECONDS
    }

    fn take_batch(&mut self) -> Option<RollupBatch> {
        self.minute_start.take()?;
        self.app_slots.clear();
        let batch = RollupBatch {
            system: std::mem::take(&mut self.system),
            apps: take_keeping_capacity(&mut self.apps),
            gpus: take_keeping_capacity(&mut self.gpus),
            devices: take_keeping_capacity(&mut self.devices),
            interfaces: take_keeping_capacity(&mut self.interfaces),
        };
        (batch.system.recorded_seconds > 0.0).then_some(batch)
    }

    fn add_system(&mut self, sample: &SystemSample) {
        let seconds = sample.elapsed_seconds;
        let minute = &mut self.system;
        minute.last_sample_unix = sample.wall_unix_seconds;
        minute.recorded_seconds += seconds;
        minute.cpu_busy_seconds += sample.cpu_busy_seconds;
        minute.memory_byte_seconds += sample.memory_used.get() as f64 * seconds;
        minute.disk_read += sample.disk_read;
        minute.disk_written += sample.disk_written;
        minute.network_received += sample.network_received;
        minute.network_transmitted += sample.network_transmitted;
        add_optional(&mut minute.energy_joules, sample.energy_joules);
        minute.pressure_level_peak = minute.pressure_level_peak.max(sample.pressure_level);
        if sample.throttled {
            minute.throttled_seconds += seconds;
        }
        if sample.on_ac == Some(true) {
            minute.on_ac_seconds += seconds;
        }
        let gauges = &mut minute.gauges;
        gauges[GaugeMetric::CpuPercent].add(f64::from(sample.cpu_percent.get()), seconds);
        gauges[GaugeMetric::MemoryBytes].add(sample.memory_used.get() as f64, seconds);
        gauges[GaugeMetric::PressurePercent]
            .add(f64::from(sample.memory_pressure_percent.get()), seconds);
        gauges[GaugeMetric::SwapBytes].add(sample.swap_used.get() as f64, seconds);
        gauges[GaugeMetric::GpuPercent].add(f64::from(sample.gpu_percent.get()), seconds);
        gauges[GaugeMetric::NpuPercent]
            .add_optional(sample.npu_percent.map(|p| f64::from(p.get())), seconds);
        gauges[GaugeMetric::CpuCelsius]
            .add_optional(sample.cpu_celsius.map(|c| f64::from(c.get())), seconds);
        gauges[GaugeMetric::HottestCelsius]
            .add_optional(sample.hottest_celsius.map(|c| f64::from(c.get())), seconds);
        gauges[GaugeMetric::FanRpm]
            .add_optional(sample.fan_rpm.map(|rpm| f64::from(rpm.get())), seconds);
        gauges[GaugeMetric::PowerWatts]
            .add_optional(sample.power_watts.map(|w| f64::from(w.get())), seconds);
        gauges[GaugeMetric::BatteryPercent]
            .add_optional(sample.battery_percent.map(|p| f64::from(p.get())), seconds);
    }

    fn add_app(&mut self, sample: &AppSample, seconds: f64) {
        let index = match self.app_slots.get(&sample.key) {
            Some(&index) => index,
            None => {
                self.apps.push(AppMinute {
                    key: sample.key.clone(),
                    name: sample.name.clone(),
                    seconds: 0.0,
                    cpu_seconds: 0.0,
                    memory_byte_seconds: 0.0,
                    peak_memory: sample.memory,
                    disk_read: Bytes::ZERO,
                    disk_written: Bytes::ZERO,
                    network_received: Bytes::ZERO,
                    network_transmitted: Bytes::ZERO,
                    gpu_seconds: 0.0,
                    energy_joules: None,
                });
                self.app_slots
                    .insert(sample.key.clone(), self.apps.len() - 1);
                self.apps.len() - 1
            }
        };
        let Some(app) = self.apps.get_mut(index) else {
            return;
        };
        app.seconds += seconds;
        app.cpu_seconds += sample.cpu_seconds;
        app.memory_byte_seconds += sample.memory.get() as f64 * seconds;
        app.peak_memory = app.peak_memory.max(sample.memory);
        app.disk_read += sample.disk_read;
        app.disk_written += sample.disk_written;
        app.network_received += sample.network_received;
        app.network_transmitted += sample.network_transmitted;
        app.gpu_seconds += sample.gpu_seconds;
        add_optional(&mut app.energy_joules, sample.energy_joules);
    }

    fn add_gpu(&mut self, sample: &GpuSample, seconds: f64) {
        let gpu = slot(&mut self.gpus, &sample.key, || GpuMinute {
            key: sample.key.clone(),
            seconds: 0.0,
            busy_seconds: 0.0,
            engines: EngineTimes::default(),
            vram_byte_seconds: 0.0,
            energy_joules: None,
            peak_celsius: None,
        });
        gpu.seconds += seconds;
        gpu.busy_seconds += sample.busy_seconds;
        gpu.engines.add(&sample.engine_busy_seconds, seconds);
        gpu.vram_byte_seconds += sample.vram_used.get() as f64 * seconds;
        add_optional(&mut gpu.energy_joules, sample.energy_joules);
        if let Some(celsius) = sample.celsius.map(|c| c.get()) {
            gpu.peak_celsius = Some(gpu.peak_celsius.map_or(celsius, |peak| peak.max(celsius)));
        }
    }
}

/// The sample covers `(wall - elapsed, wall]`; its midpoint picks the minute.
fn midpoint_unix(system: &SystemSample) -> i64 {
    system.wall_unix_seconds - (system.elapsed_seconds / 2.0).round() as i64
}

fn take_keeping_capacity<T>(items: &mut Vec<T>) -> Vec<T> {
    std::mem::replace(items, Vec::with_capacity(items.len()))
}

trait Keyed {
    fn key(&self) -> &DeviceKey;
}

impl Keyed for GpuMinute {
    fn key(&self) -> &DeviceKey {
        &self.key
    }
}

impl Keyed for DeviceSample {
    fn key(&self) -> &DeviceKey {
        &self.key
    }
}

impl Keyed for InterfaceSample {
    fn key(&self) -> &DeviceKey {
        &self.key
    }
}

/// Linear lookup: machines have a handful of GPUs, disks and interfaces.
fn slot<'a, T: Keyed>(
    items: &'a mut Vec<T>,
    key: &DeviceKey,
    create: impl FnOnce() -> T,
) -> &'a mut T {
    let index = match items.iter().position(|item| item.key() == key) {
        Some(index) => index,
        None => {
            items.push(create());
            items.len() - 1
        }
    };
    &mut items[index]
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_core::{AppKind, Percent};

    fn system_at(wall: i64, elapsed: f64) -> SystemSample {
        SystemSample {
            wall_unix_seconds: wall,
            elapsed_seconds: elapsed,
            cpu_busy_seconds: 0.25 * elapsed,
            cpu_percent: Percent::new(25.0),
            memory_used: Bytes::new(1_000),
            disk_read: Bytes::new(10),
            ..SystemSample::default()
        }
    }

    fn record(accumulator: &mut Accumulator, system: &SystemSample) -> Option<RollupBatch> {
        let app = AppSample {
            key: AppKey::new(AppKind::Desktop, "editor"),
            name: "Editor".into(),
            cpu_seconds: 0.5,
            memory: Bytes::new(500),
            disk_read: Bytes::new(1),
            disk_written: Bytes::ZERO,
            network_received: Bytes::ZERO,
            network_transmitted: Bytes::ZERO,
            gpu_seconds: 0.0,
            energy_joules: None,
        };
        let device = DeviceSample {
            key: DeviceKey::new("nvme0n1"),
            read: Bytes::new(3),
            written: Bytes::new(4),
        };
        accumulator.record(&TickSamples {
            system,
            apps: std::slice::from_ref(&app),
            gpus: &[],
            devices: std::slice::from_ref(&device),
            interfaces: &[],
        })
    }

    #[test]
    fn emits_finished_minute_on_boundary() {
        let mut accumulator = Accumulator::new();
        let base = 1_800_000_000;
        let batches: Vec<_> = (1..=120)
            .filter_map(|second| record(&mut accumulator, &system_at(base + second, 1.0)))
            .collect();
        assert_eq!(batches.len(), 1);
        let minute = &batches[0].system;
        assert_eq!(minute.start_unix, base);
        assert_eq!(minute.recorded_seconds, 60.0);
        assert_eq!(minute.cpu_busy_seconds, 15.0);
        assert_eq!(minute.disk_read, Bytes::new(600));
        assert_eq!(minute.gauges[GaugeMetric::CpuPercent].average(), Some(25.0));
        assert_eq!(batches[0].apps[0].cpu_seconds, 30.0);
        assert_eq!(batches[0].devices[0].written, Bytes::new(240));
        let rest = accumulator.flush();
        assert_eq!(rest.map(|batch| batch.system.recorded_seconds), Some(60.0));
    }

    #[test]
    fn suspended_ticks_contribute_nothing() {
        let mut accumulator = Accumulator::new();
        let base = 1_800_000_000;
        assert!(record(&mut accumulator, &system_at(base + 1, 1.0)).is_none());
        let mut asleep = system_at(base + 50, 1.0);
        asleep.suspended = true;
        assert!(record(&mut accumulator, &asleep).is_none());
        let batch = accumulator.flush();
        assert_eq!(batch.map(|batch| batch.system.recorded_seconds), Some(1.0));
    }

    #[test]
    fn wall_clock_jump_starts_new_bucket() {
        let mut accumulator = Accumulator::new();
        let base = 1_800_000_000;
        assert!(record(&mut accumulator, &system_at(base + 10, 1.0)).is_none());
        let jumped = record(&mut accumulator, &system_at(base + 20, 1.0));
        assert_eq!(jumped.map(|batch| batch.system.recorded_seconds), Some(1.0));
        let after = accumulator.flush();
        assert_eq!(after.map(|batch| batch.system.start_unix), Some(base));
    }
}
