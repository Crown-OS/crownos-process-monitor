use crownui::prelude::{Cx, Runtime, Styled, View, vstack};

use super::details::memory_bar;
use super::selection::GpuChoice;
use crate::app::AppState;
use crate::app::live::{ENGINE_SERIES, GpuHistory, recent};
use crate::components::chart::{Legend, MultiSeriesSource, Series, YRange, line_chart};
use crate::settings::ChartWindow;
use crate::util::spacing;
use crate::util::theme::Subsystem;

const CHART_HEIGHT: f32 = 170.0;
const SERIES_COUNT: usize = ENGINE_SERIES.len() + 1;
const UTILIZATION_WINDOW: ChartWindow = ChartWindow::FifteenMinutes;

fn device_series(history: &GpuHistory, samples: usize) -> [&[f32]; SERIES_COUNT] {
    std::array::from_fn(|index| match index {
        0 => recent(&history.busy, samples),
        engine => recent(&history.engines[engine - 1], samples),
    })
}

fn chosen_series(state: AppState, choice: GpuChoice) -> impl MultiSeriesSource {
    let live = state.live;
    move |runtime: &mut Runtime, sink: &mut dyn FnMut(&[&[f32]])| {
        let index = choice.index.get(runtime);
        live.with(runtime, |snapshot| match snapshot.gpu.devices.get(index) {
            Some(history) => sink(&device_series(history, UTILIZATION_WINDOW.seconds())),
            None => sink(&[]),
        });
    }
}

/// The last fifteen minutes of utilization with a line per engine, over the VRAM bar.
pub fn utilization(cx: &mut Cx, state: AppState, choice: GpuChoice) -> impl View + use<> {
    let chart = line_chart(chosen_series(state, choice), engine_lines(cx, state))
        .range(YRange::PERCENT)
        .height(CHART_HEIGHT)
        .legend(Legend::Labels);
    vstack((chart, memory_bar(cx, state, choice)))
        .gap(spacing::LOOSE)
        .w_full()
}

fn engine_lines(cx: &mut Cx, state: AppState) -> Vec<Series> {
    let mut series = Vec::with_capacity(SERIES_COUNT);
    series.push(Series {
        label: "Utilization",
        color: state.tint(cx, Subsystem::Gpu),
    });
    series.extend(
        ENGINE_SERIES
            .iter()
            .enumerate()
            .map(|(index, (_, label))| Series {
                label,
                color: state.series(cx, index),
            }),
    );
    series
}
