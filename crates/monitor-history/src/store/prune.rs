use monitor_core::util::time::{DAY_SECONDS, HOUR_SECONDS};
use rusqlite::Connection;

use crate::error::HistoryResult;
use crate::retention::Retention;
use crate::store::HistoryStore;

const SHORT_TIERS: [(&str, i64); 5] = [
    ("gpu_five_minutes", 48 * HOUR_SECONDS),
    ("system_minutes", 3 * HOUR_SECONDS),
    ("system_five_minutes", 48 * HOUR_SECONDS),
    ("gpu_engine_five_minutes", 48 * HOUR_SECONDS),
    ("app_hours", 48 * HOUR_SECONDS),
];

const RETAINED_TABLES: [&str; 10] = [
    "readings",
    "system_hours",
    "gpu_hours",
    "gpu_engine_hours",
    "device_hours",
    "interface_hours",
    "app_days",
    "alerts",
    "sessions",
    "gap_buckets",
];

const ORPHAN_APPS: &str = "DELETE FROM apps WHERE app_id NOT IN \
    (SELECT app_id FROM app_days UNION SELECT app_id FROM app_hours)";

/// Every table holding recorded data; `counter_checkpoints` is engine state.
pub const DATA_TABLES: [&str; 16] = [
    "gpu_five_minutes",
    "system_minutes",
    "system_five_minutes",
    "gpu_engine_five_minutes",
    "gpu_engine_hours",
    "app_hours",
    "readings",
    "system_hours",
    "gpu_hours",
    "device_hours",
    "interface_hours",
    "app_days",
    "alerts",
    "sessions",
    "gap_buckets",
    "apps",
];

fn delete_before(connection: &Connection, table: &str, cutoff_unix: i64) -> rusqlite::Result<()> {
    connection
        .prepare_cached(&format!("DELETE FROM {table} WHERE start_unix < ?1"))?
        .execute([cutoff_unix])
        .map(drop)
}

/// When the next hourly (short tiers) and daily (retained tables) passes are due.
#[derive(Debug)]
pub struct PruneSchedule {
    next_short_unix: i64,
    next_retained_unix: i64,
}

impl Default for PruneSchedule {
    fn default() -> Self {
        Self {
            next_short_unix: i64::MIN,
            next_retained_unix: i64::MIN,
        }
    }
}

impl PruneSchedule {
    /// Returns whether the daily pass ran, so the caller vacuums after commit.
    pub fn run_due(
        &mut self,
        connection: &Connection,
        now_unix: i64,
        retention: Retention,
    ) -> rusqlite::Result<bool> {
        if now_unix >= self.next_short_unix {
            SHORT_TIERS.iter().try_for_each(|(table, keep_seconds)| {
                delete_before(connection, table, now_unix - keep_seconds)
            })?;
            self.next_short_unix = now_unix + HOUR_SECONDS;
        }
        if now_unix < self.next_retained_unix {
            return Ok(false);
        }
        let cutoff = now_unix - retention.seconds();
        RETAINED_TABLES
            .iter()
            .try_for_each(|table| delete_before(connection, table, cutoff))?;
        connection.execute(ORPHAN_APPS, [])?;
        self.next_retained_unix = now_unix + DAY_SECONDS;
        Ok(true)
    }
}

impl HistoryStore {
    /// Prunes every tier now, regardless of schedule.
    pub fn prune_now(&mut self, now_unix: i64) -> HistoryResult<()> {
        self.prune_schedule = PruneSchedule::default();
        let transaction = self.connection.transaction()?;
        self.prune_schedule
            .run_due(&transaction, now_unix, self.retention)?;
        transaction.commit()?;
        self.app_ids.clear();
        self.connection
            .execute_batch("PRAGMA incremental_vacuum;")?;
        Ok(())
    }

    /// Clears all recorded history; the running session carries on from now.
    pub fn delete_all(&mut self, now_unix: i64) -> HistoryResult<()> {
        let transaction = self.connection.transaction()?;
        DATA_TABLES
            .iter()
            .try_for_each(|table| transaction.execute_batch(&format!("DELETE FROM {table};")))?;
        transaction.commit()?;
        self.app_ids.clear();
        if self.session_start_unix.is_some() {
            self.session_start_unix = Some(now_unix);
        }
        self.connection
            .execute_batch("PRAGMA incremental_vacuum;")?;
        Ok(())
    }
}
