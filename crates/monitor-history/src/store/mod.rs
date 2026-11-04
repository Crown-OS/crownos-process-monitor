mod alerts;
mod prune;
pub(crate) mod schema;
mod session;
pub(crate) mod table;
mod write;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use monitor_core::AppKey;
use rusqlite::Connection;

use crate::error::{HistoryError, HistoryResult};
use crate::retention::Retention;
use crate::util::paths::default_database_path;

pub use alerts::StoredAlert;
pub(crate) use alerts::load_alerts;
use prune::PruneSchedule;
pub use session::GapOutcome;
use write::WriteSql;

const BUSY_TIMEOUT: Duration = Duration::from_secs(2);
const WRITER_PRAGMAS: &str = "PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA temp_store = MEMORY;
PRAGMA cache_size = -2000;";

/// The write side of the history database. Owned by one thread (the
/// [`crate::HistoryWriter`]); readers open their own connection.
pub struct HistoryStore {
    connection: Connection,
    sql: WriteSql,
    app_ids: HashMap<AppKey, i64>,
    retention: Retention,
    session_start_unix: Option<i64>,
    prune_schedule: PruneSchedule,
}

impl HistoryStore {
    /// `$XDG_STATE_HOME/crownos-process-monitor/history.sqlite`.
    pub fn default_path() -> HistoryResult<PathBuf> {
        default_database_path().ok_or(HistoryError::NoHomeDirectory)
    }

    /// Opens or creates the database, creating parent directories.
    pub fn open(path: &Path, retention: Retention) -> HistoryResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::from_connection(Connection::open(path)?, retention)
    }

    pub fn open_in_memory(retention: Retention) -> HistoryResult<Self> {
        Self::from_connection(Connection::open_in_memory()?, retention)
    }

    fn from_connection(mut connection: Connection, retention: Retention) -> HistoryResult<Self> {
        schema::prepare_new_database(&connection)?;
        connection.busy_timeout(BUSY_TIMEOUT)?;
        connection.execute_batch(WRITER_PRAGMAS)?;
        schema::migrate(&mut connection)?;
        Ok(Self {
            connection,
            sql: WriteSql::new(),
            app_ids: HashMap::new(),
            retention,
            session_start_unix: None,
            prune_schedule: PruneSchedule::default(),
        })
    }

    pub fn set_retention(&mut self, retention: Retention) {
        self.retention = retention;
        self.prune_schedule = PruneSchedule::default();
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Runs `PRAGMA optimize` so the planner statistics stay fresh; cheap.
    pub fn close(self) -> HistoryResult<()> {
        self.connection.execute_batch("PRAGMA optimize;")?;
        self.connection.close().map_err(|(_, error)| error.into())
    }
}
