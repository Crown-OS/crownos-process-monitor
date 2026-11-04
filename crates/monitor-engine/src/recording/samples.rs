use monitor_core::{Bytes, BytesPerSecond, DeviceKey, Percent};
use monitor_hardware::PowerSource;
use monitor_hardware::gpu::{EngineClass, EngineUsage};
use monitor_history::{
    AppSample, DeviceSample, EngineBusy, GpuEngine, GpuSample, InterfaceSample, SystemSample,
    TickSamples,
};

use crate::Snapshot;

/// Reusable per-tick history samples; vectors keep their capacity.
#[derive(Debug, Default)]
pub(crate) struct SampleBuffers {
    system: SystemSample,
    apps: Vec<AppSample>,
    gpus: Vec<GpuSample>,
    devices: Vec<DeviceSample>,
    interfaces: Vec<InterfaceSample>,
    throttle_events: Option<u64>,
}

impl SampleBuffers {
    pub fn fill(&mut self, snapshot: &Snapshot) -> TickSamples<'_> {
        let seconds = if snapshot.elapsed.suspended {
            0.0
        } else {
            snapshot.elapsed.seconds
        };
        let throttle_events = snapshot.sensors.throttle_events;
        let throttled = matches!(
            (self.throttle_events, throttle_events),
            (Some(before), Some(now)) if now > before
        );
        self.throttle_events = throttle_events;
        self.system = system_sample(snapshot, seconds, throttled);
        self.fill_apps(snapshot, seconds);
        self.fill_devices(snapshot, seconds);
        TickSamples {
            system: &self.system,
            apps: &self.apps,
            gpus: &self.gpus,
            devices: &self.devices,
            interfaces: &self.interfaces,
        }
    }

    fn fill_apps(&mut self, snapshot: &Snapshot, seconds: f64) {
        self.apps.clear();
        let energy = &snapshot.energy.apps;
        self.apps.extend(
            snapshot
                .processes
                .apps
                .iter()
                .enumerate()
                .map(|(index, app)| AppSample {
                    key: app.key.clone(),
                    name: app.name.clone(),
                    cpu_seconds: share_seconds(app.cpu, seconds),
                    memory: app.memory,
                    disk_read: moved(app.disk_read, seconds),
                    disk_written: moved(app.disk_write, seconds),
                    network_received: moved(app.external.network_receive, seconds),
                    network_transmitted: moved(app.external.network_transmit, seconds),
                    gpu_seconds: share_seconds(app.external.gpu, seconds),
                    energy_joules: energy.get(index).map(|share| share.joules),
                }),
        );
    }

    fn fill_devices(&mut self, snapshot: &Snapshot, seconds: f64) {
        self.gpus.clear();
        self.gpus.extend(snapshot.gpu.devices.iter().map(|device| {
            GpuSample {
                key: device.identity.key.clone(),
                busy_seconds: device
                    .metrics
                    .utilization
                    .map_or(0.0, |busy| share_seconds(busy, seconds)),
                engine_busy_seconds: engine_busy(&device.metrics.engines, seconds),
                vram_used: device.metrics.memory.vram_used.unwrap_or(Bytes::ZERO),
                energy_joules: device
                    .metrics
                    .power_draw
                    .map(|watts| f64::from(watts.get()) * seconds),
                celsius: device.metrics.temperatures.primary(),
            }
        }));
        self.devices.clear();
        self.devices
            .extend(snapshot.disk.devices.iter().map(|disk| DeviceSample {
                key: disk.info.key.clone(),
                read: moved(disk.rates.read, seconds),
                written: moved(disk.rates.write, seconds),
            }));
        self.interfaces.clear();
        self.interfaces
            .extend(
                snapshot
                    .network
                    .interfaces
                    .iter()
                    .map(|interface| InterfaceSample {
                        key: DeviceKey::new(interface.info.name.clone()),
                        received: moved(interface.rates.receive, seconds),
                        transmitted: moved(interface.rates.transmit, seconds),
                    }),
            );
    }
}

/// The history engine family a live engine class is recorded under.
pub const fn history_engine(class: EngineClass) -> GpuEngine {
    match class {
        EngineClass::Graphics => GpuEngine::Graphics,
        EngineClass::Compute => GpuEngine::Compute,
        EngineClass::Copy => GpuEngine::Copy,
        EngineClass::Video => GpuEngine::Video,
        EngineClass::VideoEncode => GpuEngine::VideoEncode,
        EngineClass::VideoEnhance => GpuEngine::VideoEnhance,
        EngineClass::Other => GpuEngine::Other,
    }
}

fn engine_busy(engines: &EngineUsage, seconds: f64) -> EngineBusy {
    let mut busy = EngineBusy::default();
    for (class, percent) in engines.iter() {
        busy.set(history_engine(class), share_seconds(percent, seconds));
    }
    busy
}

fn system_sample(snapshot: &Snapshot, seconds: f64, throttled: bool) -> SystemSample {
    let memory = &snapshot.memory;
    let draw = snapshot.system_draw.map(|draw| draw.watts);
    let battery = snapshot.power.system_battery();
    SystemSample {
        wall_unix_seconds: snapshot
            .tick
            .map_or(0, |tick| tick.wall_unix_seconds() as i64),
        elapsed_seconds: snapshot.elapsed.seconds,
        suspended: snapshot.elapsed.suspended,
        cpu_busy_seconds: share_seconds(snapshot.cpu.total, seconds),
        cpu_percent: snapshot.cpu.total,
        memory_used: memory.used,
        memory_pressure_percent: memory
            .pressure
            .map_or(Percent::ZERO, |pressure| pressure.some.avg10),
        pressure_level: memory.pressure_level,
        swap_used: memory.swap_used,
        gpu_percent: snapshot.busiest_gpu(),
        npu_percent: snapshot
            .gpu
            .accelerators
            .iter()
            .filter_map(|accelerator| accelerator.busy)
            .reduce(max_percent),
        cpu_celsius: snapshot.sensors.cpu_temperature,
        hottest_celsius: snapshot
            .sensors
            .hottest_sensor()
            .and_then(|reading| reading.value),
        fan_rpm: snapshot
            .sensors
            .fans
            .iter()
            .filter_map(|fan| fan.rpm)
            .max_by_key(|rpm| rpm.get()),
        power_watts: draw,
        battery_percent: battery.and_then(|battery| battery.capacity),
        on_ac: match snapshot.power.source {
            PowerSource::Ac => Some(true),
            PowerSource::Battery => Some(false),
            PowerSource::Unknown => None,
        },
        throttled,
        network_received: moved(snapshot.network.totals.receive, seconds),
        network_transmitted: moved(snapshot.network.totals.transmit, seconds),
        disk_read: moved(snapshot.disk.totals.read, seconds),
        disk_written: moved(snapshot.disk.totals.write, seconds),
        energy_joules: draw.map(|watts| f64::from(watts.get()) * seconds),
    }
}

pub(crate) fn max_percent(first: Percent, second: Percent) -> Percent {
    if second.get() > first.get() {
        second
    } else {
        first
    }
}

/// Seconds of activity at `share` for `seconds`: 100 % for one second is 1.
fn share_seconds(share: Percent, seconds: f64) -> f64 {
    f64::from(share.get()) / 100.0 * seconds
}

fn moved(rate: BytesPerSecond, seconds: f64) -> Bytes {
    Bytes::new((rate.get() * seconds).max(0.0) as u64)
}

#[cfg(test)]
mod tests {
    use monitor_core::Elapsed;

    use super::*;

    #[test]
    fn suspended_ticks_move_nothing() {
        let mut snapshot = Snapshot::default();
        snapshot.cpu.total = Percent::new(50.0);
        snapshot.disk.totals.read = BytesPerSecond::new(1000.0);
        snapshot.elapsed = Elapsed {
            seconds: 2.0,
            suspended: false,
        };
        let mut buffers = SampleBuffers::default();
        let awake = buffers.fill(&snapshot).system.clone();
        assert_eq!(awake.cpu_busy_seconds, 1.0);
        assert_eq!(awake.disk_read, Bytes::new(2000));
        snapshot.elapsed.suspended = true;
        let asleep = buffers.fill(&snapshot).system.clone();
        assert!(asleep.suspended);
        assert_eq!(asleep.cpu_busy_seconds, 0.0);
        assert_eq!(asleep.disk_read, Bytes::ZERO);
    }

    #[test]
    fn engine_busy_seconds_follow_reported_engines() {
        let mut engines = EngineUsage::default();
        engines.set(EngineClass::Graphics, Percent::new(50.0));
        engines.set(EngineClass::VideoEncode, Percent::new(10.0));
        let busy = engine_busy(&engines, 2.0);
        assert_eq!(busy.get(GpuEngine::Graphics), Some(1.0));
        assert!(
            busy.get(GpuEngine::VideoEncode)
                .is_some_and(|seconds| (seconds - 0.2).abs() < 1e-6)
        );
        assert_eq!(busy.get(GpuEngine::Compute), None);
    }
}
