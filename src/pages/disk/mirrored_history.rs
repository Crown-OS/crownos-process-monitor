//! Live/History over a mirrored chart (read/write, down/up): Live shows the page's own chart,
//! History the two stored rates over the picked range.

use crownui::kit::use_theme;
use crownui::prelude::{AnyView, Cx, Runtime, Styled, View, bind, hstack, keyed, text, vstack};
use monitor_engine::history::HistoryMetric;
use monitor_engine::utc_offset_seconds;

use crate::app::AppState;
use crate::app::history_query::{
    HistoryData, SeriesRequest, history_series, pair_source, picked_range, recorded_share_text,
    recording_since_text,
};
use crate::components::chart::{Series, mirrored_chart};
use crate::components::range_picker::{HistoryRange, RangeMode, RangeSelection, range_picker};
use crate::pages::sources::rate_readout;
use crate::util::spacing;

const CAPTION_SIZE: f32 = 11.0;

fn range_label(index: Option<usize>) -> &'static str {
    index
        .and_then(|index| HistoryRange::ALL.get(index))
        .copied()
        .unwrap_or_default()
        .label()
}

/// The picker, then `live_chart` in Live mode or the stored `metrics` mirrored in History mode.
pub(in crate::pages) fn mirrored_live_or_history<V, L>(
    cx: &mut Cx,
    state: AppState,
    metrics: &'static [HistoryMetric; 2],
    series: [Series; 2],
    mut live_chart: L,
) -> impl View + use<V, L>
where
    V: View,
    L: FnMut(&mut Cx) -> V + 'static,
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
    let (data, driver) = history_series(cx, move |runtime| {
        is_history(runtime).then(|| SeriesRequest {
            metrics,
            range: picked_range(picked.get(runtime)),
        })
    });
    let chart = keyed(is_history, move |cx, history| -> AnyView {
        if history {
            mirrored_chart(pair_source(data), series)
                .readout(rate_readout(state))
                .boxed()
        } else {
            live_chart(cx).boxed()
        }
    });
    let caption = text(bind(move |runtime| {
        if !is_history(runtime) {
            return String::new();
        }
        let label = range_label(picked.get(runtime));
        data.with(runtime, |data| match data {
            HistoryData::Ready(history) => format!(
                "{} · {}",
                recorded_share_text(&history.coverage, label),
                recording_since_text(history.recording_since, utc_offset_seconds())
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
