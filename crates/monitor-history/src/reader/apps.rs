use std::sync::Arc;

use monitor_core::{AppKey, AppKind, Bytes};
use rusqlite::{Row, params};

use crate::error::HistoryResult;
use crate::range::Span;
use crate::reader::HistoryReader;
use crate::util::sql::integer_to_bytes;

const USAGE_COLUMNS: &str = "a.app_key, a.name, sum(u.seconds), sum(u.cpu_seconds), \
    sum(u.memory_byte_seconds), max(u.peak_memory_bytes), sum(u.disk_read_bytes), \
    sum(u.disk_written_bytes), sum(u.network_received_bytes), sum(u.network_transmitted_bytes), \
    sum(u.gpu_seconds), sum(u.energy_joules)";

const BUSIEST_BY_HOUR: &str = "SELECT start_unix, app_key, name, seconds, cpu_seconds, \
    memory_byte_seconds, peak_memory_bytes, disk_read_bytes, disk_written_bytes, \
    network_received_bytes, network_transmitted_bytes, gpu_seconds, energy_joules FROM ( \
        SELECT u.*, a.app_key, a.name, \
            row_number() OVER (PARTITION BY u.start_unix ORDER BY u.cpu_seconds DESC) AS rank \
        FROM app_hours u JOIN apps a ON a.app_id = u.app_id \
        WHERE u.start_unix >= ?1 AND u.start_unix < ?2) \
    WHERE rank <= ?3 ORDER BY start_unix, rank";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AppMetric {
    Cpu,
    /// Ordered by byte-seconds, i.e. average memory weighted by time.
    Memory,
    DiskRead,
    DiskWrite,
    NetworkReceive,
    NetworkTransmit,
    Gpu,
    Energy,
}

impl AppMetric {
    /// 1-based position in [`USAGE_COLUMNS`].
    const fn result_column(self) -> u8 {
        match self {
            Self::Cpu => 4,
            Self::Memory => 5,
            Self::DiskRead => 7,
            Self::DiskWrite => 8,
            Self::NetworkReceive => 9,
            Self::NetworkTransmit => 10,
            Self::Gpu => 11,
            Self::Energy => 12,
        }
    }
}

/// One app's figures over a range, from recorded time only.
#[derive(Clone, Debug, PartialEq)]
pub struct AppUsage {
    pub key: AppKey,
    pub name: Arc<str>,
    pub seconds: f64,
    /// Core-seconds, as sampled per app.
    pub cpu_seconds: f64,
    pub memory_byte_seconds: f64,
    pub peak_memory: Bytes,
    pub disk_read: Bytes,
    pub disk_written: Bytes,
    pub network_received: Bytes,
    pub network_transmitted: Bytes,
    pub gpu_seconds: f64,
    pub energy_joules: Option<f64>,
}

impl AppUsage {
    pub fn kind(&self) -> AppKind {
        self.key.kind()
    }

    pub fn average_memory(&self) -> Bytes {
        crate::util::sql::real_to_bytes(if self.seconds > 0.0 {
            self.memory_byte_seconds / self.seconds
        } else {
            0.0
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct HourRanking {
    pub start_unix: i64,
    /// Busiest first, by CPU seconds.
    pub apps: Vec<AppUsage>,
}

fn usage_from_row(row: &Row<'_>, first: usize) -> rusqlite::Result<AppUsage> {
    let raw_key: String = row.get(first)?;
    let name: String = row.get(first + 1)?;
    let bytes = |offset| row.get(first + offset).map(integer_to_bytes);
    Ok(AppUsage {
        key: AppKey::parse(&raw_key).unwrap_or_else(|| AppKey::new(AppKind::Process, &raw_key)),
        name: name.into(),
        seconds: row.get(first + 2)?,
        cpu_seconds: row.get(first + 3)?,
        memory_byte_seconds: row.get(first + 4)?,
        peak_memory: bytes(5)?,
        disk_read: bytes(6)?,
        disk_written: bytes(7)?,
        network_received: bytes(8)?,
        network_transmitted: bytes(9)?,
        gpu_seconds: row.get(first + 10)?,
        energy_joules: row.get(first + 11)?,
    })
}

impl HistoryReader {
    /// Top apps by `metric`: from `app_hours` up to 48 h, else `app_days`.
    pub fn top_apps(
        &self,
        span: Span,
        metric: AppMetric,
        limit: usize,
    ) -> HistoryResult<Vec<AppUsage>> {
        let table = span.resolution().app_table();
        let sql = format!(
            "SELECT {USAGE_COLUMNS} FROM {table} u JOIN apps a ON a.app_id = u.app_id \
             WHERE u.start_unix >= ?1 AND u.start_unix < ?2 GROUP BY u.app_id \
             ORDER BY {} DESC LIMIT ?3",
            metric.result_column()
        );
        let mut statement = self.connection().prepare_cached(&sql)?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let apps = statement
            .query_map(params![span.from_unix, span.to_unix, limit], |row| {
                usage_from_row(row, 0)
            })?
            .collect::<Result<_, _>>()?;
        Ok(apps)
    }

    /// The `per_hour` busiest apps of each recorded hour (kept for 48 h).
    pub fn busiest_apps_by_hour(
        &self,
        span: Span,
        per_hour: usize,
    ) -> HistoryResult<Vec<HourRanking>> {
        let mut statement = self.connection().prepare_cached(BUSIEST_BY_HOUR)?;
        let per_hour = i64::try_from(per_hour).unwrap_or(i64::MAX);
        let rows = statement.query_map(params![span.from_unix, span.to_unix, per_hour], |row| {
            Ok((row.get::<_, i64>(0)?, usage_from_row(row, 1)?))
        })?;
        let mut rankings: Vec<HourRanking> = Vec::new();
        for row in rows {
            let (start_unix, usage) = row?;
            match rankings.last_mut() {
                Some(ranking) if ranking.start_unix == start_unix => ranking.apps.push(usage),
                _ => rankings.push(HourRanking {
                    start_unix,
                    apps: vec![usage],
                }),
            }
        }
        Ok(rankings)
    }
}
