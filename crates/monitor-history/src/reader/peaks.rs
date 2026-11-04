use rusqlite::{OptionalExtension, params};

use crate::error::HistoryResult;
use crate::gauge::GaugeMetric;
use crate::range::Span;
use crate::reader::HistoryReader;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Peak {
    pub value: f64,
    /// Start of the five-minute reading that held the peak.
    pub at_unix: i64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Peaks {
    pub cpu_percent: Option<Peak>,
    pub memory_bytes: Option<Peak>,
    pub memory_pressure_percent: Option<Peak>,
    pub hottest_celsius: Option<Peak>,
    pub power_watts: Option<Peak>,
    pub gpu_percent: Option<Peak>,
}

impl HistoryReader {
    pub fn peaks(&self, span: Span) -> HistoryResult<Peaks> {
        Ok(Peaks {
            cpu_percent: self.peak(GaugeMetric::CpuPercent, span)?,
            memory_bytes: self.peak(GaugeMetric::MemoryBytes, span)?,
            memory_pressure_percent: self.peak(GaugeMetric::PressurePercent, span)?,
            hottest_celsius: self.peak(GaugeMetric::HottestCelsius, span)?,
            power_watts: self.peak(GaugeMetric::PowerWatts, span)?,
            gpu_percent: self.peak(GaugeMetric::GpuPercent, span)?,
        })
    }

    fn peak(&self, gauge: GaugeMetric, span: Span) -> HistoryResult<Option<Peak>> {
        let column = gauge.column();
        let sql = format!(
            "SELECT start_unix, {column}_peak FROM readings \
             WHERE start_unix >= ?1 AND start_unix < ?2 AND {column}_peak IS NOT NULL \
             ORDER BY {column}_peak DESC, start_unix LIMIT 1"
        );
        let peak = self
            .connection()
            .prepare_cached(&sql)?
            .query_row(params![span.from_unix, span.to_unix], |row| {
                Ok(Peak {
                    at_unix: row.get(0)?,
                    value: row.get(1)?,
                })
            })
            .optional()?;
        Ok(peak)
    }
}
