use crownui::prelude::{Cx, Runtime, Signal, View};
use monitor_engine::history::{
    HistoryMetric, HistoryReader, HistoryResult, RecordedShare, Resolution, SeriesPoint, Span,
    TimeRange,
};

use super::query::{HistoryData, QueryClock, history_query};
use crate::components::chart::{PairSource, SeriesSource};

/// One or more metrics over the same range, e.g. `[DiskRead, DiskWrite]` for a mirrored chart.
#[derive(Debug, Clone, PartialEq)]
pub struct SeriesRequest {
    pub metrics: &'static [HistoryMetric],
    pub range: TimeRange,
}

/// Dense chart series (NaN where nothing was recorded, so charts draw a gap), one per
/// requested metric, plus how much of the range was recorded.
#[derive(Debug, Clone, PartialEq)]
pub struct HistorySeries {
    pub series: Vec<Vec<f32>>,
    /// Recorded seconds of each bucket of the first metric; a rate times this is the bucket
    /// total (weekly bars).
    pub recorded_seconds: Vec<f32>,
    pub first_unix: i64,
    pub resolution: Resolution,
    pub coverage: RecordedShare,
    pub recording_since: Option<i64>,
}

impl HistorySeries {
    /// Bucket totals of the metrics summed: bytes per bucket for rates, for bar charts.
    pub fn bucket_totals(&self) -> Vec<f32> {
        (0..self.recorded_seconds.len())
            .map(|bucket| {
                let rate: f32 = self
                    .series
                    .iter()
                    .filter_map(|series| series.get(bucket))
                    .filter(|value| value.is_finite())
                    .sum();
                rate * self.recorded_seconds[bucket]
            })
            .collect()
    }
}

impl HistorySeries {
    /// One dense series per entry of `metrics` (all from the same span, so aligned), with the
    /// recorded seconds of the first and the range's coverage.
    pub(super) fn assemble(
        reader: &HistoryReader,
        span: Span,
        metrics: impl IntoIterator<Item = Vec<SeriesPoint>>,
    ) -> HistoryResult<Self> {
        let mut series = Vec::new();
        let mut recorded_seconds = Vec::new();
        let mut first_unix = span.from_unix;
        for points in metrics {
            if series.is_empty() {
                first_unix = points
                    .first()
                    .map_or(span.from_unix, |point| point.start_unix);
                recorded_seconds = points
                    .iter()
                    .map(|point| point.recorded_seconds as f32)
                    .collect();
            }
            series.push(chart_values(&points));
        }
        Ok(Self {
            series,
            recorded_seconds,
            first_unix,
            resolution: span.resolution(),
            coverage: reader.coverage(span)?,
            recording_since: reader.recording_since()?,
        })
    }
}

/// A point's value, NaN where nothing was recorded so charts draw a gap.
fn chart_values(points: &[SeriesPoint]) -> Vec<f32> {
    points
        .iter()
        .map(|point| point.value.map_or(f32::NAN, |value| value as f32))
        .collect()
}

/// Machine-wide series of every metric of `request`.
pub fn fetch_series(
    reader: &HistoryReader,
    request: &SeriesRequest,
    clock: QueryClock,
) -> HistoryResult<HistorySeries> {
    let span = request.range.span(clock.now_unix);
    let points = request
        .metrics
        .iter()
        .map(|metric| reader.series(*metric, span))
        .collect::<HistoryResult<Vec<_>>>()?;
    HistorySeries::assemble(reader, span, points)
}

/// The series of `request`'s metrics, refreshed when the request changes and every minute.
pub fn history_series<R>(
    cx: &mut Cx,
    request: R,
) -> (Signal<HistoryData<HistorySeries>>, impl View + use<R>)
where
    R: FnMut(&mut Runtime) -> Option<SeriesRequest> + 'static,
{
    history_query(cx, request, fetch_series)
}

/// The `index`th series of a [`history_series`] answer, for an area or bar chart.
pub fn series_source(data: Signal<HistoryData<HistorySeries>>, index: usize) -> impl SeriesSource {
    move |runtime: &mut Runtime, sink: &mut dyn FnMut(&[f32])| {
        data.with(runtime, |data| {
            sink(
                data.ready()
                    .and_then(|series| series.series.get(index))
                    .map_or(&[], Vec::as_slice),
            );
        });
    }
}

/// The first two series of a [`history_series`] answer, for a mirrored chart.
pub fn pair_source(data: Signal<HistoryData<HistorySeries>>) -> impl PairSource {
    move |runtime: &mut Runtime, sink: &mut dyn FnMut(&[f32], &[f32])| {
        data.with(runtime, |data| {
            let series = data.ready().map_or(&[][..], |series| &series.series[..]);
            let pick = |index: usize| series.get(index).map_or(&[][..], Vec::as_slice);
            sink(pick(0), pick(1));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::history_query::test_history::{
        MINUTE, RECORDED_MINUTES, TestResult, seeded_history,
    };

    #[test]
    fn series_have_points_gaps_and_a_recorded_share() -> TestResult {
        let history = seeded_history("series")?;
        let reader = HistoryReader::open(&history.database())?;
        let clock = QueryClock {
            now_unix: history.now_unix,
            utc_offset_seconds: 0,
        };
        let request = SeriesRequest {
            metrics: &[HistoryMetric::Cpu, HistoryMetric::NetworkReceive],
            range: TimeRange::Hour1,
        };
        let answer = fetch_series(&reader, &request, clock)?;
        assert_eq!(answer.series.len(), 2);
        assert_eq!(answer.resolution, Resolution::Minute);
        let cpu = &answer.series[0];
        let recorded: Vec<_> = cpu.iter().filter(|value| value.is_finite()).collect();
        assert_eq!(recorded.len(), (RECORDED_MINUTES - 1) as usize);
        assert!(recorded.iter().all(|value| (**value - 40.0).abs() < 1e-3));
        assert!(
            cpu.iter().any(|value| value.is_nan()),
            "the paused minute is a gap"
        );
        let network: Vec<_> = answer.series[1]
            .iter()
            .filter(|value| value.is_finite())
            .collect();
        assert!(network.iter().all(|rate| (**rate - 100.0).abs() < 1e-3));
        let recorded_minutes = answer.coverage.recorded_seconds / MINUTE as f64;
        assert!((recorded_minutes - (RECORDED_MINUTES - 1) as f64).abs() < 1e-6);
        assert!(answer.recording_since.is_some());
        Ok(())
    }

    #[test]
    fn the_link_caches_one_reader_across_queries() -> TestResult {
        let history = seeded_history("link")?;
        let link = crate::app::EngineLink::history_only(history.database());
        let request = SeriesRequest {
            metrics: &[HistoryMetric::Cpu],
            range: TimeRange::Hour1,
        };
        for _ in 0..2 {
            let answer = link.read(|reader| fetch_series(reader, &request, QueryClock::now()))?;
            assert!(answer.series[0].iter().any(|value| value.is_finite()));
        }
        let missing =
            crate::app::EngineLink::history_only(history.database().with_extension("gone"));
        assert!(missing.read(|reader| reader.recording_since()).is_err());
        Ok(())
    }

    #[test]
    fn weekly_bars_total_the_summed_rates() {
        let series = HistorySeries {
            series: vec![vec![1.0, f32::NAN], vec![2.0, 4.0]],
            recorded_seconds: vec![10.0, 5.0],
            first_unix: 0,
            resolution: Resolution::Week,
            coverage: RecordedShare::default(),
            recording_since: None,
        };
        assert_eq!(series.bucket_totals(), [30.0, 20.0]);
    }
}
