use monitor_core::Bytes;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::HistoryResult;
use crate::sample::KernelCounters;
use crate::store::HistoryStore;
use crate::util::sql::{bytes_to_integer, integer_to_bytes};

const LOAD_CHECKPOINT: &str = "SELECT taken_unix, boot_id, cpu_busy_seconds, disk_read_bytes, \
    disk_written_bytes, network_received_bytes, network_transmitted_bytes \
    FROM counter_checkpoints WHERE slot = 0";
const SAVE_CHECKPOINT: &str = "INSERT OR REPLACE INTO counter_checkpoints (slot, taken_unix, boot_id, \
    cpu_busy_seconds, disk_read_bytes, disk_written_bytes, network_received_bytes, \
    network_transmitted_bytes) VALUES (0, ?1, ?2, ?3, ?4, ?5, ?6, ?7)";
const INSERT_GAP: &str = "INSERT OR REPLACE INTO gap_buckets (start_unix, end_unix, estimated, \
    cpu_busy_seconds, disk_read_bytes, disk_written_bytes, network_received_bytes, \
    network_transmitted_bytes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)";
const INSERT_SESSION: &str = "INSERT INTO sessions (start_unix, end_unix, boot_id) VALUES (?1, ?1, ?2) \
    ON CONFLICT (start_unix) DO UPDATE SET boot_id = excluded.boot_id";
const LAST_SESSION_END: &str = "SELECT max(end_unix) FROM sessions";

/// What a new session found about the time since the previous one (§6.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GapOutcome {
    /// No earlier history exists.
    FirstRun,
    /// The previous session ended at the same instant.
    NoGap,
    /// Same boot: since-boot counter deltas were stored as one estimated bucket.
    Estimated,
    /// The machine rebooted: the gap is stored as "not recorded".
    NotRecorded,
}

fn checkpoint_from_row(row: &Row<'_>) -> rusqlite::Result<KernelCounters> {
    Ok(KernelCounters {
        taken_unix_seconds: row.get(0)?,
        boot_id: row.get::<_, String>(1)?.into(),
        cpu_busy_seconds_total: row.get(2)?,
        disk_read_total: integer_to_bytes(row.get(3)?),
        disk_written_total: integer_to_bytes(row.get(4)?),
        network_received_total: integer_to_bytes(row.get(5)?),
        network_transmitted_total: integer_to_bytes(row.get(6)?),
    })
}

fn save_checkpoint(connection: &Connection, counters: &KernelCounters) -> rusqlite::Result<()> {
    connection
        .prepare_cached(SAVE_CHECKPOINT)?
        .execute(params![
            counters.taken_unix_seconds,
            &*counters.boot_id,
            counters.cpu_busy_seconds_total,
            bytes_to_integer(counters.disk_read_total),
            bytes_to_integer(counters.disk_written_total),
            bytes_to_integer(counters.network_received_total),
            bytes_to_integer(counters.network_transmitted_total),
        ])?;
    Ok(())
}

/// A counter that went backwards was reset; its delta is unknown.
fn byte_delta(later: Bytes, earlier: Bytes) -> Option<i64> {
    (later >= earlier).then(|| bytes_to_integer(later.saturating_sub(earlier)))
}

fn insert_estimated_gap(
    connection: &Connection,
    previous: &KernelCounters,
    current: &KernelCounters,
) -> rusqlite::Result<()> {
    let cpu_delta = current.cpu_busy_seconds_total - previous.cpu_busy_seconds_total;
    connection.prepare_cached(INSERT_GAP)?.execute(params![
        previous.taken_unix_seconds,
        current.taken_unix_seconds,
        true,
        (cpu_delta >= 0.0).then_some(cpu_delta),
        byte_delta(current.disk_read_total, previous.disk_read_total),
        byte_delta(current.disk_written_total, previous.disk_written_total),
        byte_delta(
            current.network_received_total,
            previous.network_received_total
        ),
        byte_delta(
            current.network_transmitted_total,
            previous.network_transmitted_total
        ),
    ])?;
    Ok(())
}

fn insert_unrecorded_gap(connection: &Connection, from: i64, to: i64) -> rusqlite::Result<()> {
    let unknown: Option<i64> = None;
    connection.prepare_cached(INSERT_GAP)?.execute(params![
        from, to, false, unknown, unknown, unknown, unknown, unknown
    ])?;
    Ok(())
}

impl HistoryStore {
    /// Opens a session at `counters.taken_unix_seconds` and records the gap
    /// since the previous one: estimated from counter deltas on the same
    /// boot, "not recorded" after a reboot.
    pub fn start_session(&mut self, counters: &KernelCounters) -> HistoryResult<GapOutcome> {
        let now = counters.taken_unix_seconds;
        let transaction = self.connection.transaction()?;
        let previous = transaction
            .query_row(LOAD_CHECKPOINT, [], checkpoint_from_row)
            .optional()?;
        let last_end: Option<i64> =
            transaction.query_row(LAST_SESSION_END, [], |row| row.get(0))?;
        let outcome = match (previous, last_end) {
            (Some(previous), _) if previous.boot_id == counters.boot_id => {
                if previous.taken_unix_seconds < now {
                    insert_estimated_gap(&transaction, &previous, counters)?;
                    GapOutcome::Estimated
                } else {
                    GapOutcome::NoGap
                }
            }
            (None, None) => GapOutcome::FirstRun,
            (previous, last_end) => {
                let from = last_end
                    .or(previous.map(|checkpoint| checkpoint.taken_unix_seconds))
                    .unwrap_or(now);
                if from < now {
                    insert_unrecorded_gap(&transaction, from, now)?;
                    GapOutcome::NotRecorded
                } else {
                    GapOutcome::NoGap
                }
            }
        };
        transaction
            .prepare_cached(INSERT_SESSION)?
            .execute(params![now, &*counters.boot_id])?;
        save_checkpoint(&transaction, counters)?;
        transaction.commit()?;
        self.session_start_unix = Some(now);
        Ok(outcome)
    }

    /// Persists the latest since-boot counters (once a minute).
    pub fn checkpoint(&mut self, counters: &KernelCounters) -> HistoryResult<()> {
        save_checkpoint(&self.connection, counters)?;
        Ok(())
    }
}
