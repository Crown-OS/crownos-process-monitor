//! A seeded engine tick for page tests, applied through the same path as the live feed.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use monitor_core::{
    AppKey, AppKind, Bytes, Celsius, Coverage, DeviceKey, Megahertz, Page, Percent, Pid,
    ProcessKey, Rpm, Uid,
};
use monitor_engine::Snapshot;
use monitor_engine::hardware::gpu::{GpuClocks, GpuTemperatures, PciIdentity};
use monitor_engine::hardware::sensors::{
    FanControlSupport, FanId, FanIdentity, SensorCategory, SensorIdentity,
};
use monitor_engine::hardware::{
    BackendKind, DriverSupport, EngineClass, FanReading, GpuDevice, GpuIdentity, GpuKind,
    GpuMetrics, GpuPower, GpuProcessUsage, GpuVendor, TemperatureReading,
};
use monitor_engine::system::{
    AppRow, CacheKind, CacheLevel, CoreType, CpuInfo, CpuTopology, ExternalUsage, LogicalCpuUsage,
    MemoryCategories, MemoryMethod, MemoryModule, MemoryPressure, PressureAverages, ProcessRole,
    ProcessRow, ProcessState, RoleBreakdown, SmtState, SwapDevice, SwapKind, ZramDevice,
};

use crowntest::TestApp;
use crownui::kit::{Theme, provide_theme};
use crownui::prelude::{Cx, View, component};

use super::apps::AppListFiller;
use super::readings;
use crate::app::AppState;
use crate::app::live::LiveSnapshot;
use crate::settings::{CpuScale, MonitorSettings, Settings};

const GIB: u64 = 1 << 30;
const LOGICAL_CPUS: u32 = 8;
const TICKS: usize = 30;
pub const INTEGRATED_GPU: &str = "0000:06:00.0";
pub const DISCRETE_GPU: &str = "0000:01:00.0";

/// Which optional hardware the seeded machine has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Swap {
    Configured,
    None,
}

fn cpu_info() -> CpuInfo {
    CpuInfo {
        vendor: "AuthenticAMD".to_owned(),
        model_name: "AMD Ryzen 7 7840HS".to_owned(),
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
        caches: vec![CacheLevel {
            level: 3,
            kind: CacheKind::Unified,
            size: Bytes::new(16 << 20),
            instances: 1,
            ..CacheLevel::default()
        }],
        base_frequency: Some(Megahertz::new(3800)),
        max_frequency: Some(Megahertz::new(5100)),
        features: vec!["AVX2", "AVX-512F"],
        virtualization: Some("AMD-V"),
        ..CpuInfo::default()
    }
}

fn sensor(label: &str, celsius: f32) -> TemperatureReading {
    TemperatureReading {
        identity: Arc::new(SensorIdentity {
            key: format!("k10temp/{label}").into(),
            chip: "k10temp".into(),
            label: label.into(),
            category: SensorCategory::Cpu,
            high: None,
            critical: None,
        }),
        value: Some(Celsius::new(celsius)),
    }
}

fn fan(label: &str, rpm: u32) -> FanReading {
    FanReading {
        identity: Arc::new(FanIdentity {
            id: FanId {
                chip_directory: Arc::from(Path::new("/sys/class/hwmon/hwmon5")),
                channel: 1,
            },
            key: format!("legion/{label}").into(),
            chip: "legion_hwmon".into(),
            label: label.into(),
            min: None,
            max: None,
            control: FanControlSupport::Pwm,
        }),
        rpm: Some(Rpm::new(rpm)),
        duty: None,
    }
}

fn gpu(key: &str, name: &str, kind: GpuKind, power: GpuPower) -> GpuDevice {
    let mut metrics = GpuMetrics {
        power_state: power,
        ..GpuMetrics::default()
    };
    if power == GpuPower::Active {
        metrics.utilization = Some(Percent::new(35.0));
        metrics
            .engines
            .set(EngineClass::Graphics, Percent::new(35.0));
        metrics.engines.set(EngineClass::Video, Percent::new(8.0));
        metrics.memory.vram_used = Some(Bytes::new(GIB / 4));
        metrics.memory.vram_total = Some(Bytes::new(GIB / 2));
        metrics.memory.gtt_used = Some(Bytes::new(GIB));
        metrics.memory.gtt_total = Some(Bytes::new(8 * GIB));
        metrics.clocks = GpuClocks {
            core: Some(Megahertz::new(1800)),
            core_max: Some(Megahertz::new(2700)),
            ..GpuClocks::default()
        };
        metrics.temperatures = GpuTemperatures {
            edge: Some(Celsius::new(52.0)),
            ..GpuTemperatures::default()
        };
    }
    GpuDevice {
        identity: Arc::new(GpuIdentity {
            key: DeviceKey::new(key),
            card: "card1".into(),
            name: name.into(),
            vendor: GpuVendor::Amd,
            pci: PciIdentity {
                vendor: 0x1002,
                device: 0x15bf,
                ..PciIdentity::default()
            },
            driver: "amdgpu".into(),
            driver_version: None,
            vbios: None,
            kind,
            boot_vga: kind == GpuKind::Integrated,
            connected_displays: Arc::new(["eDP-1".into()]),
            backend: BackendKind::Amd,
            support: DriverSupport::Full,
        }),
        metrics,
    }
}

fn app(id: &str, name: &str) -> AppRow {
    AppRow {
        key: AppKey::new(AppKind::Desktop, id),
        name: name.into(),
        icon: None,
        kind: AppKind::Desktop,
        process_count: 1,
        threads: 12,
        cpu: Percent::new(20.0),
        cpu_machine: Percent::new(2.5),
        memory: Bytes::new(GIB),
        memory_method: MemoryMethod::Cgroup,
        resident: Bytes::new(GIB),
        disk_read: Default::default(),
        disk_write: Default::default(),
        external: ExternalUsage::default(),
        roles: RoleBreakdown::default(),
        scope: None,
        frozen: false,
        started_unix: 0,
        coverage: Coverage::Full,
    }
}

fn process(pid: u32, app: usize, name: &str, cpu: f32) -> ProcessRow {
    ProcessRow {
        key: ProcessKey {
            pid: Pid::new(pid),
            start_ticks: 1,
        },
        parent: Pid::new(1),
        uid: Uid::new(1000),
        name: name.into(),
        command_line: name.into(),
        executable: None,
        app,
        role: ProcessRole::default(),
        state: ProcessState::Sleeping,
        threads: 12,
        nice: 0,
        cpu: Percent::new(cpu),
        cpu_machine: Percent::new(cpu / LOGICAL_CPUS as f32),
        cpu_time: Duration::from_secs(3_725),
        resident: Bytes::new(GIB),
        proportional: Some(Bytes::new(GIB / 2)),
        unique: None,
        swap: Some(Bytes::new(GIB / 8)),
        peak_resident: None,
        minor_faults: 0,
        major_faults: 0,
        disk_read: Default::default(),
        disk_write: Default::default(),
        read_since_start: Some(Bytes::new(GIB / 4)),
        written_since_start: Some(Bytes::new(GIB / 16)),
        external: ExternalUsage {
            gpu: Percent::new(cpu / 2.0),
            gpu_memory: Bytes::new(GIB / 8),
            ..ExternalUsage::default()
        },
        started_unix: 0,
        is_new: false,
        kernel_thread: false,
        coverage: Coverage::Full,
    }
}

fn memory(snapshot: &mut Snapshot, swap: Swap) {
    let memory = &mut snapshot.memory;
    memory.total = Bytes::new(16 * GIB);
    memory.used = Bytes::new(9 * GIB);
    memory.available = Bytes::new(7 * GIB);
    memory.free = Bytes::new(3 * GIB);
    memory.categories = MemoryCategories {
        apps: Bytes::new(6 * GIB),
        shared: Bytes::new(GIB),
        cache: Bytes::new(4 * GIB),
        kernel: Bytes::new(GIB),
        compressed: Bytes::new(GIB / 2),
        free: Bytes::new(3 * GIB),
        other: Bytes::new(GIB / 2),
    };
    memory.pressure = Some(MemoryPressure {
        some: PressureAverages {
            avg10: Percent::new(3.0),
            ..PressureAverages::default()
        },
        full: PressureAverages::default(),
    });
    if swap == Swap::Configured {
        memory.swap_total = Bytes::new(8 * GIB);
        memory.swap_used = Bytes::new(GIB);
        memory.swap_devices = vec![SwapDevice {
            name: "/dev/zram0".into(),
            kind: SwapKind::Zram,
            size: Bytes::new(8 * GIB),
            used: Bytes::new(GIB),
            priority: 100,
        }];
        memory.zram = vec![ZramDevice {
            name: "zram0".into(),
            disk_size: Bytes::new(8 * GIB),
            original: Bytes::new(GIB),
            compressed: Bytes::new(GIB / 4),
            memory_used: Bytes::new(GIB / 4),
            algorithm: Some("zstd".into()),
        }];
    }
}

/// One engine tick of a two-GPU laptop, with or without swap.
pub fn seeded_tick(swap: Swap) -> Snapshot {
    let mut snapshot = Snapshot::default();
    snapshot.interest.page = Page::Gpu;
    snapshot.cpu_info = Arc::new(cpu_info());
    snapshot.cpu.total = Percent::new(28.0);
    snapshot.cpu.states.user = Percent::new(20.0);
    snapshot.cpu.states.system = Percent::new(8.0);
    snapshot.cpu.logical = (0..LOGICAL_CPUS)
        .map(|cpu| LogicalCpuUsage {
            cpu,
            usage: Percent::new(10.0 * cpu as f32),
            frequency: Some(Megahertz::new(3000 + 100 * cpu)),
        })
        .collect();
    snapshot.sensors.cpu_temperature = Some(Celsius::new(61.0));
    snapshot.sensors.temperatures = vec![sensor("Tctl", 61.0), sensor("Tccd1", 66.0)];
    snapshot.sensors.fans = vec![fan("Fan 1", 2400), fan("Fan 2", 2600)];
    memory(&mut snapshot, swap);
    snapshot.memory_modules = Arc::new([MemoryModule {
        locator: "DIMM 0".to_owned(),
        size: Bytes::new(8 * GIB),
        kind: Some("DDR5".to_owned()),
        speed_mts: Some(5600),
        manufacturer: Some("Samsung".to_owned()),
        ..MemoryModule::default()
    }]);
    snapshot.gpu.devices = vec![
        gpu(
            INTEGRATED_GPU,
            "Radeon 780M",
            GpuKind::Integrated,
            GpuPower::Active,
        ),
        gpu(
            DISCRETE_GPU,
            "GeForce RTX 4050",
            GpuKind::Discrete,
            GpuPower::Asleep,
        ),
    ];
    snapshot.gpu.processes = vec![GpuProcessUsage {
        pid: Pid::new(100),
        device: DeviceKey::new(INTEGRATED_GPU),
        utilization: Percent::new(30.0),
        engines: Default::default(),
        memory: Bytes::new(GIB / 8),
        shared_memory: Bytes::new(0),
    }];
    snapshot.gpu.process_coverage = Coverage::OwnProcesses;
    snapshot.processes.apps = vec![app("firefox", "Firefox"), app("code", "Code")];
    snapshot.processes.processes = vec![
        process(100, 0, "firefox", 30.0),
        process(200, 1, "code", 12.0),
    ];
    snapshot
}

/// A live snapshot after a few seeded ticks, as the bridge would leave it.
pub fn seeded_live(swap: Swap) -> LiveSnapshot {
    live_from(seeded_tick(swap))
}

/// A live snapshot after a few ticks of `tick`.
pub fn live_from(tick: Snapshot) -> LiveSnapshot {
    let mut live = LiveSnapshot::default();
    readings::prepare(&mut live);
    let tick = Arc::new(tick);
    for _ in 0..TICKS {
        readings::append(&mut live, &tick);
    }
    AppListFiller::new().refill(&mut live.apps, &tick.processes, CpuScale::PerMachine);
    live.details.refill(&live.apps, &tick);
    live.tick = tick;
    live
}

const WINDOW_WIDTH: f32 = 1180.0;
const WINDOW_HEIGHT: f32 = 4000.0;

/// One page alone in a tall dark window over `live`, so every section is laid out.
pub fn mount<V: View>(live: LiveSnapshot, page: fn(&mut Cx, AppState) -> V) -> TestApp {
    TestApp::with_size(
        component(move |cx| {
            provide_theme(cx, Theme::dark());
            let settings = Settings::new(cx, MonitorSettings::default(), None);
            let state = AppState::new(cx, settings, live);
            page(cx, state)
        }),
        WINDOW_WIDTH,
        WINDOW_HEIGHT,
    )
}

/// Whether any node matches; `TestApp::exists` wants exactly one.
pub fn shows(app: &mut TestApp, query: crowntest::Query) -> bool {
    app.count(query) > 0
}
