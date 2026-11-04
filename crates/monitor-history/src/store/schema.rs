use rusqlite::{Connection, Transaction};

use crate::error::{HistoryError, HistoryResult};
use crate::store::table::{GPU_ENGINE_TABLES, bucketed_tables, gpu_engine_table, gpu_table};

pub const SCHEMA_VERSION: i32 = 2;

const DIMENSION_AND_EVENT_TABLES: &str = "
CREATE TABLE apps (
    app_id INTEGER PRIMARY KEY,
    app_key TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    kind TEXT NOT NULL
) STRICT;
CREATE TABLE alerts (
    start_unix INTEGER NOT NULL,
    kind TEXT NOT NULL,
    app_key TEXT NOT NULL,
    app_name TEXT NOT NULL,
    end_unix INTEGER NOT NULL,
    peak REAL NOT NULL,
    limit_value REAL NOT NULL,
    count INTEGER NOT NULL,
    quieted INTEGER NOT NULL,
    pinned INTEGER NOT NULL,
    PRIMARY KEY (start_unix, kind, app_key)
) STRICT, WITHOUT ROWID;
CREATE TABLE sessions (
    start_unix INTEGER PRIMARY KEY,
    end_unix INTEGER NOT NULL,
    boot_id TEXT NOT NULL
) STRICT, WITHOUT ROWID;
CREATE TABLE gap_buckets (
    start_unix INTEGER PRIMARY KEY,
    end_unix INTEGER NOT NULL,
    estimated INTEGER NOT NULL,
    cpu_busy_seconds REAL,
    disk_read_bytes INTEGER,
    disk_written_bytes INTEGER,
    network_received_bytes INTEGER,
    network_transmitted_bytes INTEGER
) STRICT, WITHOUT ROWID;
CREATE TABLE counter_checkpoints (
    slot INTEGER PRIMARY KEY CHECK (slot = 0),
    taken_unix INTEGER NOT NULL,
    boot_id TEXT NOT NULL,
    cpu_busy_seconds REAL NOT NULL,
    disk_read_bytes INTEGER NOT NULL,
    disk_written_bytes INTEGER NOT NULL,
    network_received_bytes INTEGER NOT NULL,
    network_transmitted_bytes INTEGER NOT NULL
) STRICT, WITHOUT ROWID;
";

type Migration = fn(&Transaction<'_>) -> rusqlite::Result<()>;

/// Index `n` upgrades version `n` to `n + 1`. Released steps never change.
const MIGRATIONS: [Migration; SCHEMA_VERSION as usize] = [create_version_1, add_gpu_detail_tables];

fn create_version_1(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(DIMENSION_AND_EVENT_TABLES)?;
    bucketed_tables()
        .iter()
        .try_for_each(|table| transaction.execute_batch(&table.create_sql()))
}

/// Five-minute GPU rows and per-engine busy time.
fn add_gpu_detail_tables(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    std::iter::once(gpu_table("gpu_five_minutes"))
        .chain(GPU_ENGINE_TABLES.map(gpu_engine_table))
        .try_for_each(|table| transaction.execute_batch(&table.create_sql()))
}

pub fn schema_version(connection: &Connection) -> rusqlite::Result<i32> {
    connection.pragma_query_value(None, "user_version", |row| row.get(0))
}

/// `auto_vacuum` only takes effect before the first table exists, so it is
/// set while the file is still empty.
pub fn prepare_new_database(connection: &Connection) -> HistoryResult<()> {
    if schema_version(connection)? == 0 {
        connection.execute_batch("PRAGMA auto_vacuum = INCREMENTAL;")?;
    }
    Ok(())
}

pub fn migrate(connection: &mut Connection) -> HistoryResult<()> {
    let found = schema_version(connection)?;
    if found > SCHEMA_VERSION {
        return Err(HistoryError::UnsupportedSchema {
            found,
            supported: SCHEMA_VERSION,
        });
    }
    if found == SCHEMA_VERSION {
        return Ok(());
    }
    let transaction = connection.transaction()?;
    MIGRATIONS
        .iter()
        .skip(usize::try_from(found).unwrap_or(0))
        .try_for_each(|migration| migration(&transaction))?;
    transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADDED_IN_VERSION_2: [&str; 3] = [
        "gpu_engine_five_minutes",
        "gpu_engine_hours",
        "gpu_five_minutes",
    ];

    fn added_tables(connection: &Connection) -> rusqlite::Result<Vec<String>> {
        let mut statement = connection.prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' \
             AND (name LIKE 'gpu_engine%' OR name = 'gpu_five_minutes') ORDER BY name",
        )?;
        statement.query_map([], |row| row.get(0))?.collect()
    }

    #[test]
    fn version_1_upgrades_to_2_and_the_step_is_idempotent() -> HistoryResult<()> {
        let mut connection = Connection::open_in_memory()?;
        let transaction = connection.transaction()?;
        create_version_1(&transaction)?;
        transaction.pragma_update(None, "user_version", 1)?;
        transaction.commit()?;
        assert!(added_tables(&connection)?.is_empty());

        migrate(&mut connection)?;
        assert_eq!(schema_version(&connection)?, SCHEMA_VERSION);
        assert_eq!(added_tables(&connection)?, ADDED_IN_VERSION_2);

        let transaction = connection.transaction()?;
        add_gpu_detail_tables(&transaction)?;
        transaction.commit()?;
        migrate(&mut connection)?;
        assert_eq!(added_tables(&connection)?, ADDED_IN_VERSION_2);
        Ok(())
    }
}
