use monitor_core::DeviceKey;
use rusqlite::params;

use crate::error::HistoryResult;
use crate::gpu_engine::GpuEngine;
use crate::range::Span;
use crate::reader::HistoryReader;
use crate::reader::devices::gpu_tier;
use crate::reader::series::{BucketQuery, SeriesPoint};
use crate::store::table::GPU_ENGINE_TABLES;

const ENGINE_FILTER: &str = " AND gpu = ?5 AND engine = ?6";

/// One engine's busy percent over a range.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineSeries {
    pub engine: GpuEngine,
    pub points: Vec<SeriesPoint>,
}

impl HistoryReader {
    /// Busy percent of every engine `gpu` reported during `span`, in
    /// [`GpuEngine::ALL`] order, bucketed like [`Self::gpu_series`].
    pub fn gpu_engine_series(
        &self,
        gpu: &DeviceKey,
        span: Span,
    ) -> HistoryResult<Vec<EngineSeries>> {
        let resolution = span.resolution();
        let (table, step) = gpu_tier(resolution, GPU_ENGINE_TABLES);
        let mut statement = self.connection().prepare_cached(&format!(
            "SELECT DISTINCT engine FROM {table} \
             WHERE start_unix >= ?1 AND start_unix < ?2 AND gpu = ?3"
        ))?;
        let mut engines: Vec<GpuEngine> = statement
            .query_map(params![span.from_unix, span.to_unix, gpu.as_str()], |row| {
                row.get::<_, String>(0)
            })?
            .filter_map(|name| GpuEngine::from_stored_name(&name.ok()?))
            .collect();
        engines.sort_unstable();
        engines
            .into_iter()
            .map(|engine| {
                let points = self.dense_series(
                    &BucketQuery {
                        table,
                        seconds: "seconds",
                        numerator: "sum(busy_seconds)",
                        denominator: "sum(seconds)",
                        filter: ENGINE_FILTER,
                        step,
                        origin: resolution.origin_unix(),
                        scale: 100.0,
                    },
                    span,
                    &[gpu.as_str(), engine.stored_name()],
                )?;
                Ok(EngineSeries { engine, points })
            })
            .collect()
    }
}
