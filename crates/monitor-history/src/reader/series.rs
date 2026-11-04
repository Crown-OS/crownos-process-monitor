use monitor_core::util::time::align_down_from;
use rusqlite::ToSql;

use crate::error::HistoryResult;
use crate::gauge::GaugeMetric;
use crate::range::{HistoryMetric, Span};
use crate::reader::HistoryReader;

/// One chart bucket. `value` is `None` where nothing was recorded, so charts
/// draw a gap instead of interpolating.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeriesPoint {
    pub start_unix: i64,
    pub value: Option<f64>,
    /// Multiply a rate by this to get the bucket total (weekly bars).
    pub recorded_seconds: f64,
}

impl SeriesPoint {
    const fn unrecorded(start_unix: i64) -> Self {
        Self {
            start_unix,
            value: None,
            recorded_seconds: 0.0,
        }
    }
}

enum Source {
    /// A mergeable sum in the `system_*` tiers, divided by recorded seconds.
    System {
        sum_column: &'static str,
        scale: f64,
    },
    Readings(GaugeMetric),
}

impl HistoryMetric {
    const fn source(self) -> Source {
        const fn system(sum_column: &'static str, scale: f64) -> Source {
            Source::System { sum_column, scale }
        }
        match self {
            Self::Cpu => system("cpu_busy_seconds", 100.0),
            Self::Memory => system("memory_byte_seconds", 1.0),
            Self::DiskRead => system("disk_read_bytes", 1.0),
            Self::DiskWrite => system("disk_written_bytes", 1.0),
            Self::NetworkReceive => system("network_received_bytes", 1.0),
            Self::NetworkTransmit => system("network_transmitted_bytes", 1.0),
            Self::MemoryPressure => Source::Readings(GaugeMetric::PressurePercent),
            Self::Swap => Source::Readings(GaugeMetric::SwapBytes),
            Self::Gpu => Source::Readings(GaugeMetric::GpuPercent),
            Self::Power => Source::Readings(GaugeMetric::PowerWatts),
            Self::Temperature => Source::Readings(GaugeMetric::HottestCelsius),
        }
    }
}

/// One bucketed aggregate: `numerator / denominator * scale` per `step`,
/// optionally narrowed to one device, engine or app by `filter` (keys bound from `?5`).
pub(crate) struct BucketQuery<'a> {
    pub table: &'a str,
    /// The column holding each row's recorded seconds.
    pub seconds: &'a str,
    pub numerator: &'a str,
    pub denominator: &'a str,
    pub filter: &'a str,
    pub step: i64,
    pub origin: i64,
    pub scale: f64,
}

impl HistoryReader {
    /// A dense series over `span` at the span's resolution; see
    /// [`HistoryMetric`] for each metric's unit.
    pub fn series(&self, metric: HistoryMetric, span: Span) -> HistoryResult<Vec<SeriesPoint>> {
        let resolution = span.resolution();
        let origin = resolution.origin_unix();
        match metric.source() {
            Source::System { sum_column, scale } => self.dense_series(
                &BucketQuery {
                    table: resolution.system_table(),
                    seconds: "recorded_seconds",
                    numerator: &format!("sum({sum_column})"),
                    denominator: "sum(recorded_seconds)",
                    filter: "",
                    step: resolution.step_seconds(),
                    origin,
                    scale,
                },
                span,
                &[],
            ),
            Source::Readings(gauge) => {
                let column = gauge.column();
                self.dense_series(
                    &BucketQuery {
                        table: "readings",
                        seconds: "recorded_seconds",
                        numerator: &format!("sum({column}_avg * recorded_seconds)"),
                        denominator: &format!(
                            "sum(CASE WHEN {column}_avg IS NOT NULL THEN recorded_seconds END)"
                        ),
                        filter: "",
                        step: resolution.readings_step_seconds(),
                        origin,
                        scale: 1.0,
                    },
                    span,
                    &[],
                )
            }
        }
    }

    /// Buckets of `query` over `span`, with unrecorded buckets filled in.
    pub(crate) fn dense_series(
        &self,
        query: &BucketQuery<'_>,
        span: Span,
        keys: &[&str],
    ) -> HistoryResult<Vec<SeriesPoint>> {
        let BucketQuery {
            table,
            seconds,
            numerator,
            denominator,
            filter,
            step,
            origin,
            scale,
        } = *query;
        let sql = format!(
            "SELECT start_unix - ((start_unix - ?3) % ?4) AS bucket, sum({seconds}), \
             {numerator}, {denominator} FROM {table} \
             WHERE start_unix >= ?1 AND start_unix < ?2{filter} GROUP BY bucket ORDER BY bucket"
        );
        let mut statement = self.connection().prepare_cached(&sql)?;
        let read = |row: &rusqlite::Row<'_>| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, f64>(1)?,
                row.get::<_, Option<f64>>(2)?,
                row.get::<_, Option<f64>>(3)?,
            ))
        };
        let mut bound: Vec<&dyn ToSql> = vec![&span.from_unix, &span.to_unix, &origin, &step];
        bound.extend(keys.iter().map(|key| key as &dyn ToSql));
        let rows: Vec<_> = statement
            .query_map(bound.as_slice(), read)?
            .collect::<Result<_, _>>()?;
        let step_length = usize::try_from(step).unwrap_or(1);
        let first = align_down_from(span.from_unix, step, origin);
        let capacity = usize::try_from((span.to_unix - first + step - 1) / step).unwrap_or(0);
        let mut points = Vec::with_capacity(capacity);
        let mut next = first;
        for (bucket, recorded_seconds, numerator, denominator) in rows {
            points.extend(
                (next..bucket)
                    .step_by(step_length)
                    .map(SeriesPoint::unrecorded),
            );
            let value = match (numerator, denominator) {
                (Some(numerator), Some(denominator)) if denominator > 0.0 => {
                    Some(numerator / denominator * scale)
                }
                _ => None,
            };
            points.push(SeriesPoint {
                start_unix: bucket,
                value,
                recorded_seconds,
            });
            next = bucket + step;
        }
        points.extend(
            (next..span.to_unix)
                .step_by(step_length)
                .map(SeriesPoint::unrecorded),
        );
        Ok(points)
    }
}
