//! Chart sources over the live snapshot, cut to the chart window setting.

use std::hash::{DefaultHasher, Hash, Hasher};

use crownui::prelude::Runtime;

use crate::app::AppState;
use crate::app::live::{LiveSnapshot, recent};
use crate::components::chart::{MultiSeriesSource, PairSource, SeriesSource};
use crate::settings::ChartWindow;
use crate::util::format::{percent, rate};

pub type History = fn(&LiveSnapshot) -> &[f32];

/// The identity projection, for tables over the whole live snapshot.
pub fn whole_snapshot(snapshot: &LiveSnapshot) -> &LiveSnapshot {
    snapshot
}

/// The newest chart-window's worth of one series.
pub fn windowed(state: AppState, history: History) -> impl SeriesSource {
    let (live, window) = (state.live, state.chart_window);
    move |runtime, sink| {
        let samples = window.get(runtime).seconds();
        live.with(runtime, |snapshot| sink(recent(history(snapshot), samples)));
    }
}

/// Several series cut to the same window, in order.
pub fn windowed_many<const N: usize>(
    state: AppState,
    histories: [History; N],
) -> impl MultiSeriesSource {
    let (live, window) = (state.live, state.chart_window);
    move |runtime, sink| {
        let samples = window.get(runtime).seconds();
        live.with(runtime, |snapshot| {
            let series = histories.map(|history| recent(history(snapshot), samples));
            sink(&series);
        });
    }
}

/// Two series cut to the same window, for mirrored charts.
pub fn windowed_pair(state: AppState, above: History, below: History) -> impl PairSource {
    let (live, window) = (state.live, state.chart_window);
    move |runtime, sink| {
        let samples = window.get(runtime).seconds();
        live.with(runtime, |snapshot| {
            sink(
                recent(above(snapshot), samples),
                recent(below(snapshot), samples),
            );
        });
    }
}

pub fn percent_readout(_: &mut Runtime, value: f32) -> String {
    percent(value)
}

/// Formats a bytes-per-second value in the unit the settings ask for.
pub fn rate_readout(state: AppState) -> impl Fn(&mut Runtime, f32) -> String + Clone + 'static {
    let settings = state.settings;
    move |runtime, value| rate(f64::from(value), settings.get(runtime).rates)
}

/// "5 min ago", the left time label of a live chart.
pub fn window_start_label(state: AppState) -> impl FnMut(&mut Runtime) -> String + 'static {
    let window = state.chart_window;
    move |runtime| format!("{} min ago", window.get(runtime).seconds() / 60)
}

/// Every series of a fixed-size group (memory categories), cut to a fixed window.
pub fn group_over<const N: usize>(
    state: AppState,
    window: ChartWindow,
    group: fn(&LiveSnapshot) -> &[Vec<f32>; N],
) -> impl MultiSeriesSource {
    let live = state.live;
    let samples = window.seconds();
    move |runtime, sink| {
        live.with(runtime, |snapshot| {
            let series = group(snapshot)
                .each_ref()
                .map(|history| recent(history, samples));
            sink(&series);
        });
    }
}

/// A hash of the names a section is built from, so `keyed` rebuilds it only when they change.
pub fn names_key<'a>(names: impl IntoIterator<Item = &'a str>) -> u64 {
    let mut hasher = DefaultHasher::new();
    for name in names {
        name.hash(&mut hasher);
    }
    hasher.finish()
}

/// The mean and the peak of the samples in the chart window, gaps skipped; NaN when empty.
pub fn window_mean_peak(history: &[f32], samples: usize) -> (f32, f32) {
    let (sum, count, peak) = recent(history, samples)
        .iter()
        .filter(|value| value.is_finite())
        .fold((0.0, 0_u32, f32::NAN), |(sum, count, peak), value| {
            (sum + value, count + 1, peak.max(*value))
        });
    let mean = if count == 0 {
        f32::NAN
    } else {
        sum / count as f32
    };
    (mean, peak)
}

#[cfg(test)]
mod window_tests {
    use super::*;

    #[test]
    fn mean_and_peak_skip_gaps() {
        let (mean, peak) = window_mean_peak(&[90.0, 10.0, f32::NAN, 30.0], 3);
        assert_eq!((mean, peak), (20.0, 30.0));
        let (mean, peak) = window_mean_peak(&[f32::NAN], 5);
        assert!(mean.is_nan() && peak.is_nan());
    }
}
