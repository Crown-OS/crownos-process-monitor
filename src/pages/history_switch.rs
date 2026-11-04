//! The Live/History picker over a page's headline chart: Live shows the page's own chart,
//! History the stored series of one metric (weekly bars beyond 90 days).

use crownui::kit::use_theme;
use crownui::prelude::{
    AnyView, Cx, Runtime, Signal, Styled, View, bind, hstack, keyed, text, vstack,
};
use monitor_engine::history::{HistoryMetric, Resolution, TimeRange};
use monitor_engine::utc_offset_seconds;

use crate::app::AppState;
use crate::app::history_query::{
    Fetch, HistoryData, HistorySeries, SeriesRequest, fetch_series, history_query, picked_range,
    recorded_share_text, recording_since_text, series_source,
};
use crate::components::chart::{YRange, area_chart, bar_chart};
use crate::components::range_picker::{HistoryRange, RangeMode, RangeSelection, range_picker};
use crate::util::spacing;
use crate::util::theme::Subsystem;

const CAPTION_SIZE: f32 = 11.0;

/// How History mode draws its series in place of the live chart.
#[derive(Clone, Copy)]
pub struct HistoryChart {
    pub subsystem: Subsystem,
    pub range: YRange,
    pub readout: fn(&mut Runtime, f32) -> String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Live,
    History { weekly: bool },
}

fn range_label(selection: RangeSelection, runtime: &mut Runtime) -> &'static str {
    selection
        .history
        .get(runtime)
        .and_then(|index| HistoryRange::ALL.get(index))
        .copied()
        .unwrap_or_default()
        .label()
}

/// The picker, then `live_chart` in Live mode or the stored machine-wide `metric` in History
/// mode, with how much of the range was recorded.
pub fn live_or_history<V, L>(
    cx: &mut Cx,
    state: AppState,
    metric: &'static [HistoryMetric],
    history: HistoryChart,
    live_chart: L,
) -> impl View + use<V, L>
where
    V: View,
    L: FnMut(&mut Cx) -> V + 'static,
{
    let request = move |_: &mut Runtime, range| {
        Some(SeriesRequest {
            metrics: metric,
            range,
        })
    };
    live_or_history_with(cx, state, history, request, fetch_series, live_chart)
}

/// [`live_or_history`] over any stored series: `request` turns the picked range into the
/// query `fetch` answers (`None` waits).
pub fn live_or_history_with<K, R, V, L>(
    cx: &mut Cx,
    state: AppState,
    history: HistoryChart,
    request: R,
    fetch: Fetch<K, HistorySeries>,
    live_chart: L,
) -> impl View + use<K, R, V, L>
where
    K: Clone + PartialEq + Send + 'static,
    R: FnMut(&mut Runtime, TimeRange) -> Option<K> + 'static,
    V: View,
    L: FnMut(&mut Cx) -> V + 'static,
{
    let area = move |cx: &mut Cx, data| {
        area_chart(series_source(data, 0), state.tint(cx, history.subsystem))
            .range(history.range)
            .readout(history.readout)
            .boxed()
    };
    live_or_history_drawn(cx, state, history, request, fetch, live_chart, area)
}

/// [`live_or_history_with`] where `history_chart` draws the answer below 90 days, e.g. as
/// several lines; weekly bars still show the first series.
fn live_or_history_drawn<K, R, V, L, H>(
    cx: &mut Cx,
    state: AppState,
    history: HistoryChart,
    mut request: R,
    fetch: Fetch<K, HistorySeries>,
    mut live_chart: L,
    mut history_chart: H,
) -> impl View + use<K, R, V, L, H>
where
    K: Clone + PartialEq + Send + 'static,
    R: FnMut(&mut Runtime, TimeRange) -> Option<K> + 'static,
    V: View,
    L: FnMut(&mut Cx) -> V + 'static,
    H: FnMut(&mut Cx, Signal<HistoryData<HistorySeries>>) -> AnyView + 'static,
{
    let theme = use_theme(cx);
    let selection = RangeSelection {
        mode: cx.signal(0),
        live_window: state.settings.choice(
            cx,
            |settings| settings.chart_window,
            |settings, window| settings.chart_window = window,
        ),
        history: cx.signal(Some(0)),
    };
    let (mode, picked) = (selection.mode, selection.history);
    let is_history =
        move |runtime: &mut Runtime| RangeMode::from_index(mode.get(runtime)) == RangeMode::History;
    let (data, driver) = history_query(
        cx,
        move |runtime| {
            if !is_history(runtime) {
                return None;
            }
            let range = picked_range(picked.get(runtime));
            request(runtime, range)
        },
        fetch,
    );
    let tint = state.tint(cx, history.subsystem);
    let chart = keyed(
        move |runtime| {
            if is_history(runtime) {
                Shape::History {
                    weekly: picked_range(picked.get(runtime)).span(0).resolution()
                        == Resolution::Week,
                }
            } else {
                Shape::Live
            }
        },
        move |cx, shape| -> AnyView {
            match shape {
                Shape::Live => live_chart(cx).boxed(),
                Shape::History { weekly: true } => bar_chart(series_source(data, 0), tint)
                    .range(history.range)
                    .readout(move |runtime, _, value| (history.readout)(runtime, value))
                    .boxed(),
                Shape::History { weekly: false } => history_chart(cx, data),
            }
        },
    );
    let caption = text(bind(move |runtime| {
        if !is_history(runtime) {
            return String::new();
        }
        let label = range_label(selection, runtime);
        data.with(runtime, |data| match data {
            HistoryData::Ready(series) => format!(
                "{} · {}",
                recorded_share_text(&series.coverage, label),
                recording_since_text(series.recording_since, utc_offset_seconds())
            ),
            HistoryData::Loading => "Loading history…".to_owned(),
            HistoryData::Unavailable => "History is not available".to_owned(),
        })
    }))
    .text_size(CAPTION_SIZE)
    .text_color(bind(move |runtime| {
        theme.with(runtime, |theme| theme.text.muted)
    }));
    vstack((
        range_picker(selection, vstack(())),
        chart,
        hstack((caption, driver)),
    ))
    .gap(spacing::ROOMY)
    .w_full()
}
