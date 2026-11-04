use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use monitor_core::{AppKey, AppKind, Bytes, Celsius, DeviceKey, Percent, Watts};
use monitor_history::{
    Accumulator, AppSample, DeviceSample, EngineBusy, GpuEngine, GpuSample, InterfaceSample,
    PressureLevel, RollupBatch, SystemSample, TickSamples,
};

pub type TestResult = Result<(), Box<dyn std::error::Error>>;

pub const TICK_SECONDS: i64 = 5;
/// The accumulator files a tick under its midpoint, `wall - round(5 / 2)`.
const TICK_MIDPOINT_OFFSET: i64 = 3;
/// An hour-aligned instant in 2027.
pub const BASE_UNIX: i64 = 1_800_000_000 - 1_800_000_000 % 86_400;
pub const SPIKE_MINUTES: std::ops::Range<i64> = 30..35;
pub const PAUSED_MINUTES: std::ops::Range<i64> = 60..70;
pub const MEMORY_BYTES: u64 = 4 << 30;
pub const DISK_READ_PER_TICK: u64 = 1_000;

static NEXT_DIRECTORY: AtomicU32 = AtomicU32::new(0);

pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(label: &str) -> std::io::Result<Self> {
        let unique = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "monitor-history-{label}-{}-{unique}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn database(&self) -> PathBuf {
        self.0.join("state").join("history.sqlite")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn cpu_percent_for_minute(minute: i64) -> f32 {
    if SPIKE_MINUTES.contains(&minute) {
        95.0
    } else {
        50.0
    }
}

pub fn system_sample(wall: i64, suspended: bool) -> SystemSample {
    let minute = (wall - TICK_MIDPOINT_OFFSET - BASE_UNIX).div_euclid(60);
    let cpu = cpu_percent_for_minute(minute);
    SystemSample {
        wall_unix_seconds: wall,
        elapsed_seconds: TICK_SECONDS as f64,
        suspended,
        cpu_busy_seconds: f64::from(cpu) / 100.0 * TICK_SECONDS as f64,
        cpu_percent: Percent::new(cpu),
        memory_used: Bytes::new(MEMORY_BYTES),
        memory_pressure_percent: Percent::new(2.0),
        pressure_level: PressureLevel::Normal,
        gpu_percent: Percent::new(10.0),
        hottest_celsius: Some(Celsius::new(
            60.0 + f32::from(SPIKE_MINUTES.contains(&minute)) * 30.0,
        )),
        power_watts: Some(Watts::new(15.0)),
        on_ac: Some(true),
        disk_read: Bytes::new(DISK_READ_PER_TICK),
        energy_joules: Some(75.0),
        ..SystemSample::default()
    }
}

pub fn app_sample(kind: AppKind, id: &str, name: &str, cpu_seconds: f64) -> AppSample {
    AppSample {
        key: AppKey::new(kind, id),
        name: name.into(),
        cpu_seconds,
        memory: Bytes::new(100 << 20),
        disk_read: Bytes::new(10),
        disk_written: Bytes::new(20),
        network_received: Bytes::new(30),
        network_transmitted: Bytes::new(40),
        gpu_seconds: 0.0,
        energy_joules: None,
    }
}

/// Two hours of five-second ticks from [`BASE_UNIX`] with a CPU spike and a
/// ten-minute suspend; returns every finished minute.
pub fn two_hours_of_batches() -> Vec<RollupBatch> {
    let apps = [
        app_sample(AppKind::Desktop, "org.example.Editor", "Editor, Pro", 2.0),
        app_sample(AppKind::Service, "indexer.service", "Indexer", 0.5),
    ];
    let mut engine_busy_seconds = EngineBusy::default();
    engine_busy_seconds.set(GpuEngine::Graphics, 0.5);
    engine_busy_seconds.set(GpuEngine::Video, 1.0);
    let gpus = [GpuSample {
        key: DeviceKey::new("0000:06:00.0"),
        busy_seconds: 0.5,
        engine_busy_seconds,
        vram_used: Bytes::new(1 << 30),
        energy_joules: Some(10.0),
        celsius: Some(Celsius::new(55.0)),
    }];
    let devices = [DeviceSample {
        key: DeviceKey::new("nvme0n1"),
        read: Bytes::new(DISK_READ_PER_TICK),
        written: Bytes::ZERO,
    }];
    let interfaces = [InterfaceSample {
        key: DeviceKey::new("wlo1"),
        received: Bytes::new(500),
        transmitted: Bytes::new(50),
    }];
    let mut accumulator = Accumulator::new();
    let mut batches = Vec::new();
    let end = BASE_UNIX + 2 * 3600;
    let mut wall = BASE_UNIX + TICK_SECONDS;
    while wall <= end {
        let paused_from = BASE_UNIX + PAUSED_MINUTES.start * 60;
        let paused_to = BASE_UNIX + PAUSED_MINUTES.end * 60;
        if (paused_from..paused_to).contains(&wall) {
            wall = paused_to;
        }
        let system = system_sample(wall, wall == paused_to);
        batches.extend(accumulator.record(&TickSamples {
            system: &system,
            apps: &apps,
            gpus: &gpus,
            devices: &devices,
            interfaces: &interfaces,
        }));
        wall += TICK_SECONDS;
    }
    batches.extend(accumulator.flush());
    batches
}
