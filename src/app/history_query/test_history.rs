//! A seeded history database shared by the query tests.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use monitor_core::util::time::unix_now;
use monitor_core::{AppKey, AppKind, Bytes};
use monitor_engine::history::{AppMinute, HistoryStore, Retention, RollupBatch, SystemMinute};

pub type TestResult = Result<(), Box<dyn std::error::Error>>;
pub const MINUTE: i64 = 60;
pub const RECORDED_MINUTES: i64 = 30;
const PAUSED_MINUTE: i64 = 10;

static NEXT_DIRECTORY: AtomicU32 = AtomicU32::new(0);

/// A history database in a fresh temporary folder, removed on drop.
pub struct TempHistory {
    directory: PathBuf,
    pub now_unix: i64,
}

impl TempHistory {
    pub fn database(&self) -> PathBuf {
        self.directory.join("history.sqlite")
    }
}

impl Drop for TempHistory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn minute(start_unix: i64, cpu_percent: f64) -> RollupBatch {
    RollupBatch {
        system: SystemMinute {
            start_unix,
            last_sample_unix: start_unix + MINUTE - 1,
            recorded_seconds: MINUTE as f64,
            cpu_busy_seconds: cpu_percent / 100.0 * MINUTE as f64,
            memory_byte_seconds: (2_u64 << 30) as f64 * MINUTE as f64,
            network_received: Bytes::new(6_000),
            ..SystemMinute::default()
        },
        apps: vec![AppMinute {
            key: AppKey::new(AppKind::Desktop, "org.example.Editor"),
            name: "Editor".into(),
            seconds: MINUTE as f64,
            cpu_seconds: 12.0,
            memory_byte_seconds: 0.0,
            peak_memory: Bytes::new(0),
            disk_read: Bytes::new(0),
            disk_written: Bytes::new(0),
            network_received: Bytes::new(0),
            network_transmitted: Bytes::new(0),
            gpu_seconds: 0.0,
            energy_joules: None,
        }],
        ..RollupBatch::default()
    }
}

/// Thirty recorded minutes ending now at 40 % CPU, minute 10 left unrecorded.
pub fn seeded_history(label: &str) -> Result<TempHistory, Box<dyn std::error::Error>> {
    let unique = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "process-monitor-{label}-{}-{unique}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory)?;
    let now = unix_now();
    let first = now - now.rem_euclid(MINUTE) - RECORDED_MINUTES * MINUTE;
    let history = TempHistory {
        directory,
        now_unix: now,
    };
    let mut store = HistoryStore::open(&history.database(), Retention::Days30)?;
    for index in (0..RECORDED_MINUTES).filter(|index| *index != PAUSED_MINUTE) {
        store.write_batch(&minute(first + index * MINUTE, 40.0))?;
    }
    store.close()?;
    Ok(history)
}
