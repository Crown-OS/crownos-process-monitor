//! History for the CrownOS process monitor: per-tick samples are folded into
//! mergeable minute buckets ([`Accumulator`]), written by one background
//! thread into SQLite tiers ([`HistoryWriter`]) and queried by range from a
//! separate read-only connection ([`HistoryReader`]). History exists only
//! while the app runs; closed time is covered by session gap buckets.

mod accumulator;
mod batch;
mod error;
mod export;
mod gauge;
mod gpu_engine;
mod range;
mod reader;
mod retention;
mod sample;
mod store;
mod util;
mod writer;

pub use accumulator::Accumulator;
pub use batch::{AppMinute, GpuMinute, RollupBatch, SystemMinute};
pub use error::{HistoryError, HistoryResult};
pub use gauge::{Gauge, GaugeMetric, Gauges};
pub use gpu_engine::{EngineBusy, EngineTime, EngineTimes, GpuEngine};
pub use range::{HistoryMetric, Resolution, Span, TimeRange};
pub use reader::{
    AppMetric, AppUsage, CounterTotals, EngineSeries, GapBucket, GpuMetric, HistoryReader,
    HourRanking, InterfaceTotals, LimitKind, LimitStretch, LimitThresholds, Peak, Peaks,
    RecordedShare, SeriesPoint, Totals,
};
pub use retention::Retention;
pub use sample::{
    AppSample, DeviceSample, GpuSample, InterfaceSample, KernelCounters, PressureLevel,
    SystemSample, TickSamples,
};
pub use store::{GapOutcome, HistoryStore, StoredAlert};
pub use writer::HistoryWriter;
