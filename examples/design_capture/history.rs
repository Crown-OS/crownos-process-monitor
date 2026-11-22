//! A day of recorded minutes, so History mode, "today" figures and period totals have data.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use monitor_core::util::time::unix_now;
use monitor_core::{AppKey, AppKind, Bytes, DeviceKey};
use monitor_engine::history::{
    AppMinute, GaugeMetric, Gauges, HistoryStore, InterfaceSample, Retention, RollupBatch,
    SystemMinute,
};

use crate::wave::Wave;

const MINUTE: i64 = 60;
const RECORDED_MINUTES: i64 = 26 * 60;
const MEMORY_TOTAL: f64 = (32_u64 << 30) as f64;
const MIB: f64 = (1 << 20) as f64;
/// The laptop slept over lunch: History shows an honest gap.
const ASLEEP_MINUTES: std::ops::Range<i64> = 300..345;

const HISTORY_APPS: [(AppKind, &str, &str, f64); 4] = [
    (AppKind::Desktop, "firefox", "Firefox", 9.0),
    (AppKind::Terminal, "crownterm", "Terminal", 14.0),
    (AppKind::Desktop, "code", "Code", 6.0),
    (AppKind::Flatpak, "com.spotify.Client", "Spotify", 2.0),
];

fn gauges(cpu: f64, memory_percent: f64, celsius: f64, watts: f64) -> Gauges {
    let mut gauges = Gauges::default();
    let mut set = |metric, value| gauges[metric].add(value, MINUTE as f64);
    set(GaugeMetric::CpuPercent, cpu);
    set(
        GaugeMetric::MemoryBytes,
        MEMORY_TOTAL * memory_percent / 100.0,
    );
    set(GaugeMetric::PressurePercent, cpu / 20.0);
    set(GaugeMetric::SwapBytes, 1.2 * (1_u64 << 30) as f64);
    set(GaugeMetric::GpuPercent, cpu * 0.6);
    set(GaugeMetric::CpuCelsius, celsius);
    set(GaugeMetric::HottestCelsius, celsius + 4.0);
    set(GaugeMetric::FanRpm, 1800.0 + celsius * 18.0);
    set(GaugeMetric::PowerWatts, watts);
    set(GaugeMetric::BatteryPercent, 78.0);
    gauges
}

fn minute(start_unix: i64, wave: &mut Wave) -> RollupBatch {
    wave.tick += 1;
    let seconds = MINUTE as f64;
    let cpu = f64::from(wave.at(22.0, 16.0, 90.0));
    let memory_percent = f64::from(wave.at(48.0, 8.0, 300.0));
    let celsius = f64::from(wave.at(60.0, 9.0, 120.0));
    let watts = f64::from(wave.at(17.0, 6.0, 70.0));
    let mut megabytes =
        |base, amplitude, period| f64::from(wave.at(base, amplitude, period)) * MIB * seconds;
    let received = megabytes(0.24, 0.3, 45.0);
    let transmitted = megabytes(0.03, 0.04, 60.0);
    let read = megabytes(1.2, 2.0, 50.0);
    let written = megabytes(0.6, 1.2, 80.0);
    RollupBatch {
        system: SystemMinute {
            start_unix,
            last_sample_unix: start_unix + MINUTE - 1,
            recorded_seconds: seconds,
            cpu_busy_seconds: cpu / 100.0 * seconds,
            memory_byte_seconds: MEMORY_TOTAL * memory_percent / 100.0 * seconds,
            disk_read: Bytes::new(read as u64),
            disk_written: Bytes::new(written as u64),
            network_received: Bytes::new(received as u64),
            network_transmitted: Bytes::new(transmitted as u64),
            energy_joules: Some(watts * seconds),
            gauges: gauges(cpu, memory_percent, celsius, watts),
            ..SystemMinute::default()
        },
        apps: HISTORY_APPS
            .iter()
            .map(|&(kind, id, name, share)| AppMinute {
                key: AppKey::new(kind, id),
                name: name.into(),
                seconds,
                cpu_seconds: cpu * share / 100.0 * seconds / 4.0,
                memory_byte_seconds: share * 80.0 * MIB * seconds,
                peak_memory: Bytes::new((share * 90.0 * MIB) as u64),
                disk_read: Bytes::new((read * share / 40.0) as u64),
                disk_written: Bytes::new((written * share / 40.0) as u64),
                network_received: Bytes::new((received * share / 20.0) as u64),
                network_transmitted: Bytes::new((transmitted * share / 20.0) as u64),
                gpu_seconds: share * seconds / 30.0,
                energy_joules: None,
            })
            .collect(),
        interfaces: vec![
            InterfaceSample {
                key: DeviceKey::new("wlo1"),
                received: Bytes::new((received * 0.96) as u64),
                transmitted: Bytes::new((transmitted * 0.96) as u64),
            },
            InterfaceSample {
                key: DeviceKey::new("tailscale0"),
                received: Bytes::new((received * 0.04) as u64),
                transmitted: Bytes::new((transmitted * 0.04) as u64),
            },
        ],
        ..RollupBatch::default()
    }
}

/// Writes the last 26 hours, minute by minute, to a fresh database under `directory`.
pub fn seed(directory: &Path) -> Result<PathBuf> {
    let _ = std::fs::remove_dir_all(directory);
    let database = directory.join("history.sqlite");
    let mut store = HistoryStore::open(&database, Retention::Days30)
        .context("cannot create the sample history")?;
    let now = unix_now();
    let first = now - now.rem_euclid(MINUTE) - RECORDED_MINUTES * MINUTE;
    let mut wave = Wave::default();
    for index in (0..RECORDED_MINUTES).filter(|index| !ASLEEP_MINUTES.contains(index)) {
        store
            .write_batch(&minute(first + index * MINUTE, &mut wave))
            .context("cannot write the sample history")?;
    }
    store.close().context("cannot close the sample history")?;
    Ok(database)
}
