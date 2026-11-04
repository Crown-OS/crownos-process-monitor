mod apps;
mod devices;
mod engines;
mod limits;
mod peaks;
mod series;
mod totals;

use std::path::Path;
use std::time::Duration;

use monitor_core::Bytes;
use rusqlite::{Connection, OpenFlags, Row, params};

use crate::error::HistoryResult;
use crate::range::Span;
use crate::store::StoredAlert;
use crate::util::sql::integer_to_bytes;

pub use apps::{AppMetric, AppUsage, HourRanking};
pub use devices::{GpuMetric, InterfaceTotals};
pub use engines::EngineSeries;
pub use limits::{LimitKind, LimitStretch, LimitThresholds};
pub use peaks::{Peak, Peaks};
pub use series::SeriesPoint;
pub use totals::{CounterTotals, Totals};

const BUSY_TIMEOUT: Duration = Duration::from_secs(2);
const READER_PRAGMAS: &str = "PRAGMA query_only = ON;
PRAGMA temp_store = MEMORY;
PRAGMA cache_size = -1000;";
const SELECT_GAPS: &str = "SELECT start_unix, end_unix, estimated, cpu_busy_seconds, disk_read_bytes, \
    disk_written_bytes, network_received_bytes, network_transmitted_bytes FROM gap_buckets \
    WHERE end_unix > ?1 AND start_unix < ?2 ORDER BY start_unix";

/// Recorded share of a range, e.g. "recorded 9 h 40 m of 24 h".
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RecordedShare {
    pub recorded_seconds: f64,
    pub span_seconds: i64,
}

impl RecordedShare {
    pub fn fraction(&self) -> f64 {
        if self.span_seconds <= 0 {
            0.0
        } else {
            (self.recorded_seconds / self.span_seconds as f64).min(1.0)
        }
    }
}

/// Time the app was not running. `estimated` gaps carry since-boot counter
/// deltas (unknown fields are `None`); the others are "not recorded".
#[derive(Clone, Debug, PartialEq)]
pub struct GapBucket {
    pub start_unix: i64,
    pub end_unix: i64,
    pub estimated: bool,
    pub cpu_busy_seconds: Option<f64>,
    pub disk_read: Option<Bytes>,
    pub disk_written: Option<Bytes>,
    pub network_received: Option<Bytes>,
    pub network_transmitted: Option<Bytes>,
}

fn gap_from_row(row: &Row<'_>) -> rusqlite::Result<GapBucket> {
    let bytes = |index| {
        row.get::<_, Option<i64>>(index)
            .map(|v| v.map(integer_to_bytes))
    };
    Ok(GapBucket {
        start_unix: row.get(0)?,
        end_unix: row.get(1)?,
        estimated: row.get(2)?,
        cpu_busy_seconds: row.get(3)?,
        disk_read: bytes(4)?,
        disk_written: bytes(5)?,
        network_received: bytes(6)?,
        network_transmitted: bytes(7)?,
    })
}

/// A read-only connection for range queries. It is `Send`, so it can live
/// on any background thread; it never blocks the writer (WAL).
pub struct HistoryReader {
    connection: Connection,
}

impl HistoryReader {
    /// Opens an existing database; spawn the writer first so it exists.
    pub fn open(path: &Path) -> HistoryResult<Self> {
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.busy_timeout(BUSY_TIMEOUT)?;
        connection.execute_batch(READER_PRAGMAS)?;
        Ok(Self { connection })
    }

    pub(crate) fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn coverage(&self, span: Span) -> HistoryResult<RecordedShare> {
        let table = span.resolution().system_table();
        let recorded_seconds = self.connection.query_row(
            &format!(
                "SELECT coalesce(sum(recorded_seconds), 0.0) FROM {table} \
                 WHERE start_unix >= ?1 AND start_unix < ?2"
            ),
            params![span.from_unix, span.to_unix],
            |row| row.get(0),
        )?;
        Ok(RecordedShare {
            recorded_seconds,
            span_seconds: span.seconds(),
        })
    }

    /// Start of the oldest kept session, i.e. since when history exists.
    pub fn recording_since(&self) -> HistoryResult<Option<i64>> {
        Ok(self
            .connection
            .query_row("SELECT min(start_unix) FROM sessions", [], |row| row.get(0))?)
    }

    pub fn gaps(&self, span: Span) -> HistoryResult<Vec<GapBucket>> {
        let mut statement = self.connection.prepare_cached(SELECT_GAPS)?;
        let gaps = statement
            .query_map(params![span.from_unix, span.to_unix], gap_from_row)?
            .collect::<Result<_, _>>()?;
        Ok(gaps)
    }

    pub fn alerts(&self, span: Span) -> HistoryResult<Vec<StoredAlert>> {
        crate::store::load_alerts(&self.connection, span)
    }
}
