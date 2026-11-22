//! A Ryzen laptop with an integrated Radeon and a sleeping GeForce, sampled once a second for
//! half an hour of deterministic readings.

use std::path::Path;
use std::sync::Arc;

use monitor_core::util::time::unix_now;
use monitor_core::{Bytes, Celsius, Coverage, DeviceKey, Megahertz, Percent, Pid, Rpm, Watts};
use monitor_engine::Snapshot;
use monitor_engine::hardware::gpu::{GpuClocks, GpuTemperatures, PciIdentity};
use monitor_engine::hardware::power::{BatteryIdentity, BatteryScope, WattHours};
use monitor_engine::hardware::sensors::{
    FanControlSupport, FanId, FanIdentity, SensorCategory, SensorIdentity,
};
use monitor_engine::hardware::{
    BackendKind, BatteryReading, BatteryStatus, DriverSupport, EngineClass, FanReading, GpuDevice,
    GpuIdentity, GpuKind, GpuMetrics, GpuPower, GpuProcessUsage, GpuVendor, PowerSource,
    SystemDraw, TemperatureReading,
};
use monitor_engine::system::{
    CacheKind, CacheLevel, CoreType, CpuInfo, CpuPolicy, CpuTopology, EventRates, FrequencySummary,
    LoadAverage, LogicalCpuUsage, MemoryCategories, MemoryModule, MemoryPressure, PressureAverages,
    SmtState, SwapDevice, SwapKind, ZramDevice,
};

use crate::apps::{self, LOGICAL_CPUS};
use crate::devices::{self, DiskLoad, NetworkLoad};
use crate::wave::Wave;

const GIB: u64 = 1 << 30;
const MIB: f64 = (1 << 20) as f64;
const MEMORY_TOTAL: u64 = 32 * GIB;
/// Thirty minutes at one sample per second: every live chart is full.
pub const SAMPLES: usize = 30 * 60;
pub const INTEGRATED_GPU: &str = "0000:06:00.0";
pub const DISCRETE_GPU: &str = "0000:01:00.0";

fn cpu_info() -> CpuInfo {
    CpuInfo {
        vendor: "AuthenticAMD".to_owned(),
        model_name: "AMD Ryzen 7 7840HS w/ Radeon 780M Graphics".to_owned(),
        microarchitecture: Some("Zen 4"),
        family: 0x19,
        model: 0x74,
        stepping: 1,
        microcode: Some("0xa704104".to_owned()),
        sockets: 1,
        cores: LOGICAL_CPUS / 2,
        threads: LOGICAL_CPUS,
        smt: SmtState::On,
        cpus: (0..LOGICAL_CPUS)
            .map(|cpu| CpuTopology {
                cpu,
                online: true,
                core: cpu % (LOGICAL_CPUS / 2),
                core_type: CoreType::Uniform,
                ..CpuTopology::default()
            })
            .collect(),
        caches: vec![
            CacheLevel {
                level: 2,
                kind: CacheKind::Unified,
                size: Bytes::new(1 << 20),
                instances: 8,
                ..CacheLevel::default()
            },
            CacheLevel {
                level: 3,
                kind: CacheKind::Unified,
                size: Bytes::new(16 << 20),
                instances: 1,
                ..CacheLevel::default()
            },
        ],
        base_frequency: Some(Megahertz::new(3800)),
        max_frequency: Some(Megahertz::new(5100)),
        features: vec!["AVX2", "AVX-512F", "SHA", "VAES"],
        virtualization: Some("AMD-V"),
        ..CpuInfo::default()
    }
}

fn sensor(chip: &str, label: &str, category: SensorCategory, celsius: f32) -> TemperatureReading {
    TemperatureReading {
        identity: Arc::new(SensorIdentity {
            key: format!("{chip}/{label}").into(),
            chip: chip.into(),
            label: label.into(),
            category,
            high: None,
            critical: (category == SensorCategory::Cpu).then(|| Celsius::new(100.0)),
        }),
        value: Some(Celsius::new(celsius)),
    }
}

fn fan(label: &str, channel: u8, rpm: u32) -> FanReading {
    FanReading {
        identity: Arc::new(FanIdentity {
            id: FanId {
                chip_directory: Arc::from(Path::new("/sys/class/hwmon/hwmon5")),
                channel,
            },
            key: format!("legion_hwmon/{label}").into(),
            chip: "legion_hwmon".into(),
            label: label.into(),
            min: Some(Rpm::new(0)),
            max: Some(Rpm::new(5200)),
            control: FanControlSupport::Pwm,
        }),
        rpm: Some(Rpm::new(rpm)),
        duty: None,
    }
}

fn gpu(
    key: &str,
    (name, vendor, kind): (&str, GpuVendor, GpuKind),
    busy: Option<f32>,
) -> GpuDevice {
    let mut metrics = GpuMetrics {
        power_state: if busy.is_some() {
            GpuPower::Active
        } else {
            GpuPower::Asleep
        },
        ..GpuMetrics::default()
    };
    if let Some(busy) = busy {
        metrics.utilization = Some(Percent::new(busy));
        metrics
            .engines
            .set(EngineClass::Graphics, Percent::new(busy));
        metrics
            .engines
            .set(EngineClass::Video, Percent::new(busy * 0.3));
        metrics
            .engines
            .set(EngineClass::Compute, Percent::new(busy * 0.15));
        metrics.memory.vram_used = Some(Bytes::new(GIB / 4 + (busy * 6.0 * MIB as f32) as u64));
        metrics.memory.vram_total = Some(Bytes::new(GIB / 2));
        metrics.memory.gtt_used = Some(Bytes::new(GIB + GIB / 3));
        metrics.memory.gtt_total = Some(Bytes::new(16 * GIB));
        metrics.clocks = GpuClocks {
            core: Some(Megahertz::new(800 + (busy * 19.0) as u32)),
            core_max: Some(Megahertz::new(2700)),
            ..GpuClocks::default()
        };
        metrics.temperatures = GpuTemperatures {
            edge: Some(Celsius::new(46.0 + busy * 0.2)),
            ..GpuTemperatures::default()
        };
        metrics.power_draw = Some(Watts::new(4.0 + busy * 0.12));
    }
    let (pci_vendor, driver, backend) = match vendor {
        GpuVendor::Nvidia => (0x10de, "nvidia", BackendKind::Nvidia),
        _ => (0x1002, "amdgpu", BackendKind::Amd),
    };
    GpuDevice {
        identity: Arc::new(GpuIdentity {
            key: DeviceKey::new(key),
            card: if kind == GpuKind::Integrated {
                "card1"
            } else {
                "card0"
            }
            .into(),
            name: name.into(),
            vendor,
            pci: PciIdentity {
                vendor: pci_vendor,
                device: 0x15bf,
                ..PciIdentity::default()
            },
            driver: driver.into(),
            driver_version: None,
            vbios: None,
            kind,
            boot_vga: kind == GpuKind::Integrated,
            connected_displays: Arc::from(if kind == GpuKind::Integrated {
                vec!["eDP-1".into()]
            } else {
                Vec::new()
            }),
            backend,
            support: DriverSupport::Full,
        }),
        metrics,
    }
}

fn memory(snapshot: &mut Snapshot, used_percent: f32) {
    let used = (MEMORY_TOTAL as f32 * used_percent / 100.0) as u64;
    let cache = 7 * GIB;
    let memory = &mut snapshot.memory;
    memory.total = Bytes::new(MEMORY_TOTAL);
    memory.used = Bytes::new(used);
    memory.available = Bytes::new(MEMORY_TOTAL - used);
    memory.free = Bytes::new(MEMORY_TOTAL - used - cache);
    memory.categories = MemoryCategories {
        apps: Bytes::new(used - 2 * GIB - GIB / 2),
        shared: Bytes::new(GIB),
        cache: Bytes::new(cache),
        kernel: Bytes::new(GIB + GIB / 2),
        compressed: Bytes::new(GIB / 2),
        free: Bytes::new(MEMORY_TOTAL - used - cache),
        other: Bytes::new(GIB / 4),
    };
    memory.dirty = Bytes::new(48 << 20);
    memory.pressure = Some(MemoryPressure {
        some: PressureAverages {
            avg10: Percent::new(0.8),
            ..PressureAverages::default()
        },
        full: PressureAverages::default(),
    });
    memory.swap_total = Bytes::new(16 * GIB);
    memory.swap_used = Bytes::new(GIB + GIB / 5);
    memory.swap_devices = vec![SwapDevice {
        name: "/dev/zram0".into(),
        kind: SwapKind::Zram,
        size: Bytes::new(16 * GIB),
        used: Bytes::new(GIB + GIB / 5),
        priority: 100,
    }];
    memory.zram = vec![ZramDevice {
        name: "zram0".into(),
        disk_size: Bytes::new(16 * GIB),
        original: Bytes::new(GIB + GIB / 5),
        compressed: Bytes::new(GIB / 3),
        memory_used: Bytes::new(GIB / 3),
        algorithm: Some("zstd".into()),
    }];
}

fn battery(draw: f32) -> BatteryReading {
    BatteryReading {
        identity: Arc::new(BatteryIdentity {
            name: "BAT0".into(),
            model: Some("L22M4PC1".into()),
            manufacturer: Some("Celxpert".into()),
            technology: Some("Li-poly".into()),
            scope: BatteryScope::System,
        }),
        status: BatteryStatus::Discharging,
        capacity: Some(Percent::new(78.0)),
        power: Some(Watts::new(draw)),
        energy_now: Some(WattHours::new(62.4)),
        energy_full: Some(WattHours::new(80.0)),
        energy_full_design: Some(WattHours::new(84.0)),
        health: Some(Percent::new(95.0)),
        cycle_count: Some(142),
        time_to_empty: Some(std::time::Duration::from_secs(4 * 3600 + 12 * 60)),
        time_to_full: None,
    }
}

/// The parts of a tick that never change.
fn base() -> Snapshot {
    let mut snapshot = Snapshot {
        cpu_info: Arc::new(cpu_info()),
        memory_modules: Arc::from(
            (0..2)
                .map(|index| MemoryModule {
                    index,
                    locator: format!("DIMM {index}"),
                    size: Bytes::new(16 * GIB),
                    kind: Some("DDR5".to_owned()),
                    form_factor: Some("SODIMM".to_owned()),
                    speed_mts: Some(5600),
                    configured_speed_mts: Some(5600),
                    manufacturer: Some("Samsung".to_owned()),
                    ..MemoryModule::default()
                })
                .collect::<Vec<_>>(),
        ),
        ..Snapshot::default()
    };
    snapshot.cpu.policy = CpuPolicy {
        driver: Some("amd-pstate-epp".into()),
        governor: Some("powersave".into()),
        energy_preference: Some("balance_performance".into()),
        boost: Some(true),
    };
    snapshot.gpu.process_coverage = Coverage::OwnProcesses;
    snapshot.power.source = PowerSource::Battery;
    snapshot
}

fn tick(base: &Snapshot, wave: &mut Wave, now: u64) -> Snapshot {
    wave.tick += 1;
    let mut snapshot = base.clone();
    let user = wave.at(16.0, 9.0, 40.0);
    let system = wave.at(5.0, 2.5, 25.0);
    let iowait = wave.at(0.8, 1.0, 15.0);
    let cpu = &mut snapshot.cpu;
    cpu.total = Percent::new((user + system + iowait).min(100.0));
    cpu.states.user = Percent::new(user);
    cpu.states.system = Percent::new(system);
    cpu.states.iowait = Percent::new(iowait);
    cpu.states.idle = Percent::new((100.0 - user - system - iowait).max(0.0));
    cpu.logical = (0..LOGICAL_CPUS)
        .map(|index| LogicalCpuUsage {
            cpu: index,
            usage: Percent::new((user * (0.4 + 1.2 * wave.noise()) + system).min(100.0)),
            frequency: Some(Megahertz::new(2900 + (wave.noise() * 2000.0) as u32)),
        })
        .collect();
    let average = wave.at(3600.0, 700.0, 20.0) as u32;
    cpu.frequency = FrequencySummary {
        average: Some(Megahertz::new(average)),
        min: Some(Megahertz::new(average.saturating_sub(800))),
        max: Some(Megahertz::new((average + 1100).min(5100))),
    };
    cpu.load = LoadAverage {
        one: user / 6.0,
        five: user / 7.0,
        fifteen: user / 8.0,
    };
    cpu.pressure = Some(PressureAverages {
        avg10: Percent::new(iowait * 2.0),
        ..PressureAverages::default()
    });
    cpu.events = EventRates {
        context_switches: user * 900.0,
        interrupts: user * 400.0,
        soft_interrupts: user * 120.0,
        forks: user / 4.0,
    };
    cpu.procs_running = (user / 6.0).round() as u32 + 1;
    cpu.threads = 1874;
    cpu.uptime = std::time::Duration::from_secs(3 * 3600 + 25 * 60);

    let package = wave.at(62.0, 7.0, 60.0);
    snapshot.sensors.cpu_temperature = Some(Celsius::new(package));
    snapshot.sensors.temperatures = vec![
        sensor("k10temp", "Tctl", SensorCategory::Cpu, package),
        sensor("k10temp", "Tccd1", SensorCategory::Cpu, package + 4.0),
        sensor("amdgpu", "edge", SensorCategory::Gpu, package - 9.0),
        sensor("nvme", "Composite", SensorCategory::Drive, 41.0),
        sensor("iwlwifi_1", "temp1", SensorCategory::Wireless, 44.0),
        sensor("acpitz", "temp1", SensorCategory::Acpi, 51.0),
    ];
    snapshot.sensors.hottest = Some(1);
    snapshot.sensors.fans = vec![
        fan("CPU Fan", 1, (1800.0 + package * 18.0) as u32),
        fan("GPU Fan", 2, (1600.0 + package * 15.0) as u32),
    ];

    memory(&mut snapshot, wave.at(46.0, 4.0, 120.0).min(100.0));

    let graphics = wave.at(14.0, 10.0, 30.0);
    snapshot.gpu.devices = vec![
        gpu(
            INTEGRATED_GPU,
            ("Radeon 780M", GpuVendor::Amd, GpuKind::Integrated),
            Some(graphics),
        ),
        gpu(
            DISCRETE_GPU,
            (
                "GeForce RTX 4060 Laptop GPU",
                GpuVendor::Nvidia,
                GpuKind::Discrete,
            ),
            None,
        ),
    ];
    snapshot.gpu.processes = [(2366, 0.7), (2851, 0.2), (2581, 0.1)]
        .into_iter()
        .map(|(pid, share)| GpuProcessUsage {
            pid: Pid::new(pid),
            device: DeviceKey::new(INTEGRATED_GPU),
            utilization: Percent::new(graphics * share),
            engines: Default::default(),
            memory: Bytes::new((180.0 * f64::from(share) * MIB) as u64),
            shared_memory: Bytes::new((90.0 * f64::from(share) * MIB) as u64),
        })
        .collect();

    snapshot.disk = devices::disk(DiskLoad {
        read: f64::from(wave.at(1.4, 2.2, 18.0)) * MIB,
        write: f64::from(wave.at(0.7, 1.5, 22.0)) * MIB,
    });
    snapshot.network = devices::network(NetworkLoad {
        download: f64::from(wave.at(2.6, 3.0, 12.0)) * MIB,
        upload: f64::from(wave.at(0.3, 0.4, 16.0)) * MIB,
        vpn: f64::from(wave.at(0.08, 0.06, 20.0)) * MIB,
    });

    let draw = wave.at(17.0, 5.0, 35.0);
    snapshot.power.batteries = vec![battery(draw)];
    snapshot.system_draw = Some(SystemDraw {
        watts: Watts::new(draw),
        coverage: Coverage::Full,
    });
    snapshot.processes = apps::table(wave, now);
    snapshot.sequence = wave.tick;
    snapshot
}

/// Half an hour of ticks ending now.
pub fn ticks() -> Vec<Arc<Snapshot>> {
    let base = base();
    let mut wave = Wave::default();
    let now = unix_now() as u64;
    (0..SAMPLES)
        .map(|_| Arc::new(tick(&base, &mut wave, now)))
        .collect()
}
