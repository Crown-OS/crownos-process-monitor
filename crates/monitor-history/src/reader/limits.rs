use monitor_core::util::time::FIVE_MINUTES_SECONDS;
use rusqlite::params;

use crate::error::HistoryResult;
use crate::range::Span;
use crate::reader::HistoryReader;
use crate::sample::PressureLevel;

const SELECT_LIMIT_READINGS: &str = "SELECT start_unix, cpu_percent_avg, gpu_percent_avg, \
    pressure_level_peak, hottest_celsius_avg, throttled_seconds FROM readings \
    WHERE start_unix >= ?1 AND start_unix < ?2 ORDER BY start_unix";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LimitKind {
    Cpu,
    Gpu,
    MemoryPressure,
    Temperature,
    Throttling,
}

impl LimitKind {
    const ALL: [Self; 5] = [
        Self::Cpu,
        Self::Gpu,
        Self::MemoryPressure,
        Self::Temperature,
        Self::Throttling,
    ];
}

/// Applied to five-minute averages, so brief spikes don't count.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LimitThresholds {
    pub cpu_percent: f64,
    pub gpu_percent: f64,
    pub celsius: f64,
}

impl Default for LimitThresholds {
    fn default() -> Self {
        Self {
            cpu_percent: 90.0,
            gpu_percent: 90.0,
            celsius: 90.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LimitStretch {
    pub kind: LimitKind,
    pub from_unix: i64,
    pub to_unix: i64,
}

impl LimitStretch {
    pub const fn seconds(&self) -> i64 {
        self.to_unix - self.from_unix
    }
}

struct LimitReading {
    start_unix: i64,
    cpu_percent: Option<f64>,
    gpu_percent: Option<f64>,
    pressure_level: PressureLevel,
    hottest_celsius: Option<f64>,
    throttled_seconds: f64,
}

impl LimitReading {
    fn crosses(&self, kind: LimitKind, thresholds: &LimitThresholds) -> bool {
        let at_least = |value: Option<f64>, limit| value.is_some_and(|value| value >= limit);
        match kind {
            LimitKind::Cpu => at_least(self.cpu_percent, thresholds.cpu_percent),
            LimitKind::Gpu => at_least(self.gpu_percent, thresholds.gpu_percent),
            LimitKind::MemoryPressure => self.pressure_level != PressureLevel::Normal,
            LimitKind::Temperature => at_least(self.hottest_celsius, thresholds.celsius),
            LimitKind::Throttling => self.throttled_seconds > 0.0,
        }
    }
}

impl HistoryReader {
    /// Runs of consecutive five-minute readings over a limit, merged per
    /// kind and ordered by start.
    pub fn limit_stretches(
        &self,
        span: Span,
        thresholds: &LimitThresholds,
    ) -> HistoryResult<Vec<LimitStretch>> {
        let mut statement = self.connection().prepare_cached(SELECT_LIMIT_READINGS)?;
        let readings = statement.query_map(params![span.from_unix, span.to_unix], |row| {
            Ok(LimitReading {
                start_unix: row.get(0)?,
                cpu_percent: row.get(1)?,
                gpu_percent: row.get(2)?,
                pressure_level: PressureLevel::from_code(row.get(3)?),
                hottest_celsius: row.get(4)?,
                throttled_seconds: row.get(5)?,
            })
        })?;
        let mut open: [Option<LimitStretch>; LimitKind::ALL.len()] = [None; LimitKind::ALL.len()];
        let mut stretches = Vec::new();
        for reading in readings {
            let reading = reading?;
            let end = reading.start_unix + FIVE_MINUTES_SECONDS;
            for (kind, slot) in LimitKind::ALL.into_iter().zip(&mut open) {
                if !reading.crosses(kind, thresholds) {
                    stretches.extend(slot.take());
                    continue;
                }
                match slot {
                    Some(stretch) if stretch.to_unix == reading.start_unix => stretch.to_unix = end,
                    _ => {
                        stretches.extend(slot.replace(LimitStretch {
                            kind,
                            from_unix: reading.start_unix,
                            to_unix: end,
                        }));
                    }
                }
            }
        }
        stretches.extend(open.into_iter().flatten());
        stretches.sort_by_key(|stretch| (stretch.from_unix, stretch.kind as u8));
        Ok(stretches)
    }
}
