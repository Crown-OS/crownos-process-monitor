use std::sync::Arc;

use monitor_core::util::time::{DAY_SECONDS, FIVE_MINUTES_SECONDS, HOUR_SECONDS};
use monitor_core::{AppKey, Bytes, DeviceKey};
use rusqlite::params;

use crate::error::HistoryResult;
use crate::range::{Resolution, Span};
use crate::reader::HistoryReader;
use crate::reader::series::{BucketQuery, SeriesPoint};
use crate::store::table::GPU_TABLES;
use crate::util::sql::integer_to_bytes;

const APP_FILTER: &str = " AND app_id = (SELECT app_id FROM apps WHERE app_key = ?5)";
const GPU_FILTER: &str = " AND gpu = ?5";
const INTERFACE_TOTALS: &str = "SELECT interface, coalesce(sum(received_bytes), 0), \
    coalesce(sum(transmitted_bytes), 0) FROM interface_hours \
    WHERE start_unix >= ?1 AND start_unix < ?2 GROUP BY interface ORDER BY interface";

/// What a per-GPU series plots.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GpuMetric {
    /// Percent of recorded time the GPU was busy.
    Busy,
    /// Average VRAM in use, bytes.
    VramUsed,
}

impl GpuMetric {
    const fn sum_column(self) -> &'static str {
        match self {
            Self::Busy => "sum(busy_seconds)",
            Self::VramUsed => "sum(vram_byte_seconds)",
        }
    }

    const fn scale(self) -> f64 {
        match self {
            Self::Busy => 100.0,
            Self::VramUsed => 1.0,
        }
    }
}

/// Bytes one network interface moved over a range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterfaceTotals {
    pub interface: Arc<str>,
    pub received: Bytes,
    pub transmitted: Bytes,
}

/// Per-device and per-app tiers are hourly at best.
const fn hourly_step(resolution: Resolution) -> i64 {
    match resolution {
        Resolution::Minute | Resolution::FiveMinutes | Resolution::Hour => HOUR_SECONDS,
        Resolution::Week => resolution.step_seconds(),
    }
}

/// A GPU tier and its bucket step: `five_minutes` up to 48 h, `hours` beyond (weekly
/// buckets past 90 days).
pub(crate) const fn gpu_tier(
    resolution: Resolution,
    [five_minutes, hours]: [&'static str; 2],
) -> (&'static str, i64) {
    match resolution {
        Resolution::Minute | Resolution::FiveMinutes => (five_minutes, FIVE_MINUTES_SECONDS),
        Resolution::Hour | Resolution::Week => (hours, hourly_step(resolution)),
    }
}

/// `app_hours` up to 48 h, `app_days` beyond.
const fn app_step(resolution: Resolution) -> i64 {
    match resolution {
        Resolution::Minute | Resolution::FiveMinutes => HOUR_SECONDS,
        Resolution::Hour => DAY_SECONDS,
        Resolution::Week => resolution.step_seconds(),
    }
}

impl HistoryReader {
    /// One GPU's series: five-minute buckets up to 48 h, hourly up to 90 days, then weekly.
    pub fn gpu_series(
        &self,
        gpu: &DeviceKey,
        metric: GpuMetric,
        span: Span,
    ) -> HistoryResult<Vec<SeriesPoint>> {
        let resolution = span.resolution();
        let (table, step) = gpu_tier(resolution, GPU_TABLES);
        self.dense_series(
            &BucketQuery {
                table,
                seconds: "seconds",
                numerator: metric.sum_column(),
                denominator: "sum(seconds)",
                filter: GPU_FILTER,
                step,
                origin: resolution.origin_unix(),
                scale: metric.scale(),
            },
            span,
            &[gpu.as_str()],
        )
    }

    /// One app's average memory in bytes per hour up to 48 h, per day beyond.
    pub fn app_memory_series(&self, app: &AppKey, span: Span) -> HistoryResult<Vec<SeriesPoint>> {
        let resolution = span.resolution();
        self.dense_series(
            &BucketQuery {
                table: resolution.app_table(),
                seconds: "seconds",
                numerator: "sum(memory_byte_seconds)",
                denominator: "sum(seconds)",
                filter: APP_FILTER,
                step: app_step(resolution),
                origin: resolution.origin_unix(),
                scale: 1.0,
            },
            span,
            &[app.as_str()],
        )
    }

    /// Bytes each interface moved over `span`, by name; recorded time only.
    pub fn interface_totals(&self, span: Span) -> HistoryResult<Vec<InterfaceTotals>> {
        let mut statement = self.connection().prepare_cached(INTERFACE_TOTALS)?;
        let totals = statement
            .query_map(params![span.from_unix, span.to_unix], |row| {
                Ok(InterfaceTotals {
                    interface: row.get::<_, String>(0)?.into(),
                    received: integer_to_bytes(row.get(1)?),
                    transmitted: integer_to_bytes(row.get(2)?),
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(totals)
    }
}
