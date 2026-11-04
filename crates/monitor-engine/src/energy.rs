//! Power by App: each tick's energy split over apps by their share of CPU
//! and GPU time. Always an estimate ([`Coverage::Estimated`]): idle power is
//! handed to whoever was busy, and an integrated GPU's part of the package
//! is guessed from its busy share.
//!
//! - **CPU pool:** RAPL package energy (through the helper), or, on battery
//!   without RAPL, the battery discharge minus discrete GPU power.
//! - **Integrated GPU:** its power is inside the package, so the pool is
//!   split between CPU and iGPU in proportion to their busy percentages.
//! - **Discrete GPUs:** each device's own power draw.
//!
//! Apps get the CPU part by their share of CPU time and each GPU part by
//! their share of that device's utilization (or of its memory while every
//! client is idle). Energy nobody was busy for stays unattributed.

use monitor_core::{Coverage, DeviceKey, Watts};
use monitor_hardware::{GpuDevice, GpuKind, GpuSnapshot, PowerSnapshot, PowerSource, RaplReader};
use monitor_system::ProcessTable;

use crate::Snapshot;

/// One app's part of this tick's energy; see [`EnergyEstimate::apps`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AppEnergy {
    /// Average over the tick.
    pub watts: Watts,
    /// Consumed during the tick.
    pub joules: f64,
}

/// Where the tick's energy figure comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum EnergySource {
    #[default]
    Unavailable,
    /// RAPL package counters (via the privileged helper).
    Rapl,
    /// Battery discharge power, on battery only.
    Battery,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EnergyEstimate {
    /// Parallel to `processes.apps` (same index); empty while unavailable.
    pub apps: Vec<AppEnergy>,
    /// The pool attributed by CPU time.
    pub cpu_watts: Watts,
    /// The pools attributed by GPU time: discrete GPUs plus the iGPU share.
    pub gpu_watts: Watts,
    pub source: EnergySource,
    /// `Estimated` when `apps` is filled, else `Unavailable`.
    pub coverage: Coverage,
}

/// Reusable per-app weights, so a tick allocates nothing.
#[derive(Default)]
pub(crate) struct EnergyEstimator {
    weights: Vec<f64>,
}

impl EnergyEstimator {
    pub fn estimate(&mut self, snapshot: &mut Snapshot) {
        let seconds = if snapshot.elapsed.suspended {
            0.0
        } else {
            snapshot.elapsed.seconds
        };
        let Snapshot {
            energy,
            processes,
            gpu,
            cpu,
            power,
            ..
        } = snapshot;
        energy.apps.clear();
        let Some((source, package)) = package_watts(power, gpu) else {
            energy.cpu_watts = Watts::new(0.0);
            energy.gpu_watts = Watts::new(0.0);
            energy.source = EnergySource::Unavailable;
            energy.coverage = Coverage::Unavailable;
            return;
        };
        let integrated = IntegratedGpu::of(gpu, f64::from(cpu.total.get()));
        let cpu_pool = package * (1.0 - integrated.share);
        energy
            .apps
            .resize(processes.apps.len(), AppEnergy::default());
        split_by_cpu_time(&mut energy.apps, processes, cpu_pool);
        let mut gpu_pools = 0.0;
        for device in &gpu.devices {
            let pool = integrated.device_pool(device, package);
            if pool <= 0.0 {
                continue;
            }
            gpu_pools += pool;
            self.weigh_clients(processes, gpu, &device.identity.key);
            let total: f64 = self.weights.iter().sum();
            if total > 0.0 {
                for (share, weight) in energy.apps.iter_mut().zip(&self.weights) {
                    share.watts = share.watts + watts(pool * weight / total);
                }
            }
        }
        for share in &mut energy.apps {
            share.joules = f64::from(share.watts.get()) * seconds;
        }
        energy.cpu_watts = watts(cpu_pool);
        energy.gpu_watts = watts(gpu_pools);
        energy.source = source;
        energy.coverage = Coverage::Estimated;
    }

    /// Each app's utilization of `device`, or its memory there while every
    /// client is idle.
    fn weigh_clients(&mut self, processes: &ProcessTable, gpu: &GpuSnapshot, device: &DeviceKey) {
        self.weights.clear();
        self.weights.resize(processes.apps.len(), 0.0);
        let clients = || {
            gpu.processes
                .iter()
                .filter(move |usage| usage.device == *device)
                .filter_map(|usage| {
                    let row = &processes.processes[processes.index_of(usage.pid)?];
                    Some((row.app, usage))
                })
        };
        for (app, usage) in clients() {
            if let Some(weight) = self.weights.get_mut(app) {
                *weight += f64::from(usage.utilization.get());
            }
        }
        if self.weights.iter().all(|weight| *weight <= 0.0) {
            for (app, usage) in clients() {
                if let Some(weight) = self.weights.get_mut(app) {
                    *weight += (usage.memory + usage.shared_memory).get() as f64;
                }
            }
        }
    }
}

/// How busy the integrated GPU was, and its guessed share of the package.
struct IntegratedGpu {
    busy: f64,
    share: f64,
}

impl IntegratedGpu {
    fn of(gpu: &GpuSnapshot, cpu_busy: f64) -> Self {
        let busy = gpu
            .devices
            .iter()
            .filter(|device| device.identity.kind == GpuKind::Integrated)
            .filter_map(|device| device.metrics.utilization)
            .map(|busy| f64::from(busy.get()))
            .fold(0.0, f64::max);
        let share = if busy > 0.0 {
            busy / (busy + cpu_busy)
        } else {
            0.0
        };
        Self { busy, share }
    }

    /// The watts `device` is attributed out of a `package` pool.
    fn device_pool(&self, device: &GpuDevice, package: f64) -> f64 {
        match device.identity.kind {
            GpuKind::Integrated if self.busy > 0.0 => {
                let busy = device.metrics.utilization.map_or(0.0, |busy| busy.get());
                package * self.share * f64::from(busy) / self.busy
            }
            GpuKind::Integrated => 0.0,
            GpuKind::Discrete => device
                .metrics
                .power_draw
                .map_or(0.0, |draw| f64::from(draw.get())),
        }
    }
}

fn split_by_cpu_time(apps: &mut [AppEnergy], processes: &ProcessTable, cpu_pool: f64) {
    let cpu_total: f64 = processes
        .apps
        .iter()
        .map(|app| f64::from(app.cpu.get()))
        .sum();
    if cpu_total <= 0.0 {
        return;
    }
    for (share, app) in apps.iter_mut().zip(&processes.apps) {
        share.watts = watts(cpu_pool * f64::from(app.cpu.get()) / cpu_total);
    }
}

fn watts(value: f64) -> Watts {
    Watts::new(value as f32)
}

/// The package pool and its source, or `None` without an energy source.
fn package_watts(power: &PowerSnapshot, gpu: &GpuSnapshot) -> Option<(EnergySource, f64)> {
    if power.rapl_coverage == Coverage::Full
        && let Some(watts) = RaplReader::package_power(&power.rapl)
    {
        return Some((EnergySource::Rapl, f64::from(watts.get())));
    }
    if power.source != PowerSource::Battery {
        return None;
    }
    let battery = power.system_battery()?.power?;
    let discrete: f32 = gpu
        .devices
        .iter()
        .filter(|device| device.identity.kind == GpuKind::Discrete)
        .filter_map(|device| device.metrics.power_draw)
        .map(Watts::get)
        .sum();
    Some((
        EnergySource::Battery,
        f64::from((battery.get() - discrete).max(0.0)),
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use monitor_core::{AppKey, AppKind, Bytes, BytesPerSecond, Elapsed, Percent};
    use monitor_hardware::power::RaplPower;
    use monitor_hardware::{BatteryReading, BatteryStatus};
    use monitor_system::{AppRow, ExternalUsage, MemoryMethod, RoleBreakdown};

    use super::*;

    fn app(id: &str, cpu: f32) -> AppRow {
        AppRow {
            key: AppKey::new(AppKind::Process, id),
            name: Arc::from(id),
            icon: None,
            kind: AppKind::Process,
            process_count: 1,
            threads: 1,
            cpu: Percent::new(cpu),
            cpu_machine: Percent::new(cpu),
            memory: Bytes::ZERO,
            memory_method: MemoryMethod::Rss,
            resident: Bytes::ZERO,
            disk_read: BytesPerSecond::ZERO,
            disk_write: BytesPerSecond::ZERO,
            external: ExternalUsage::default(),
            roles: RoleBreakdown::default(),
            scope: None,
            frozen: false,
            started_unix: 0,
            coverage: Coverage::Full,
        }
    }

    fn busy_snapshot() -> Snapshot {
        let mut snapshot = Snapshot {
            elapsed: Elapsed {
                seconds: 2.0,
                suspended: false,
            },
            ..Snapshot::default()
        };
        snapshot.cpu.total = Percent::new(30.0);
        snapshot.processes.apps = vec![app("busy", 20.0), app("light", 10.0)];
        snapshot
    }

    #[test]
    fn splits_package_energy_by_cpu_share() {
        let mut snapshot = busy_snapshot();
        snapshot.power.rapl = vec![RaplPower {
            name: Arc::from("package-0"),
            is_package: true,
            watts: Watts::new(9.0),
        }];
        snapshot.power.rapl_coverage = Coverage::Full;
        EnergyEstimator::default().estimate(&mut snapshot);
        let energy = &snapshot.energy;
        assert_eq!(energy.source, EnergySource::Rapl);
        assert_eq!(energy.coverage, Coverage::Estimated);
        assert_eq!(energy.apps[0].watts, Watts::new(6.0));
        assert_eq!(energy.apps[1].watts, Watts::new(3.0));
        assert_eq!(energy.apps[0].joules, 12.0);
        assert_eq!(snapshot.app_energy(1).map(|share| share.joules), Some(6.0));
    }

    #[test]
    fn uses_battery_discharge_only_on_battery() {
        let mut snapshot = busy_snapshot();
        EnergyEstimator::default().estimate(&mut snapshot);
        assert_eq!(snapshot.energy.coverage, Coverage::Unavailable);
        assert!(snapshot.energy.apps.is_empty());

        snapshot.power.source = PowerSource::Battery;
        snapshot.power.batteries = vec![BatteryReading {
            identity: Arc::new(monitor_hardware::power::BatteryIdentity {
                name: Arc::from("BAT1"),
                model: None,
                manufacturer: None,
                technology: None,
                scope: monitor_hardware::power::BatteryScope::System,
            }),
            status: BatteryStatus::Discharging,
            capacity: None,
            power: Some(Watts::new(15.0)),
            energy_now: None,
            energy_full: None,
            energy_full_design: None,
            health: None,
            cycle_count: None,
            time_to_empty: None,
            time_to_full: None,
        }];
        EnergyEstimator::default().estimate(&mut snapshot);
        assert_eq!(snapshot.energy.source, EnergySource::Battery);
        assert_eq!(snapshot.energy.cpu_watts, Watts::new(15.0));
        assert_eq!(snapshot.energy.apps[1].watts, Watts::new(5.0));
    }
}
