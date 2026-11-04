use crownui::kit::{ToggleItem, select, toggle_group};
use crownui::prelude::{Signal, Styled, View, bind, component, hstack, show, spacer};

use crate::settings::{ChartWindow, Choice};
use crate::util::spacing;

/// The history spans a chart can show; beyond 90 days a bar stands for a week.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HistoryRange {
    #[default]
    OneHour,
    TwelveHours,
    Day,
    Week,
    Month,
    Quarter,
    HalfYear,
    Year,
}

impl HistoryRange {
    pub const ALL: [Self; 8] = [
        Self::OneHour,
        Self::TwelveHours,
        Self::Day,
        Self::Week,
        Self::Month,
        Self::Quarter,
        Self::HalfYear,
        Self::Year,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::OneHour => "1 h",
            Self::TwelveHours => "12 h",
            Self::Day => "24 h",
            Self::Week => "7 d",
            Self::Month => "30 d",
            Self::Quarter => "90 d",
            Self::HalfYear => "6 mo",
            Self::Year => "1 y",
        }
    }
}

/// Live or History, as the first segment of the picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeMode {
    Live,
    History,
}

impl RangeMode {
    pub const fn from_index(index: usize) -> Self {
        if index == 0 {
            Self::Live
        } else {
            Self::History
        }
    }
}

/// The picker's state, owned by the page: `mode` indexes Live/History, `live_window` the
/// [`ChartWindow`] options and `history` the [`HistoryRange`] options.
#[derive(Debug, Clone, Copy)]
pub struct RangeSelection {
    pub mode: Signal<usize>,
    pub live_window: Signal<usize>,
    pub history: Signal<Option<usize>>,
}

/// Live (5/15/30 min) or History (1 h … 1 y), with an `export` slot at the trailing edge.
pub fn range_picker(selection: RangeSelection, export: impl View) -> impl View {
    component(move |cx| {
        let options = cx.signal(
            HistoryRange::ALL
                .iter()
                .map(|range| range.label().to_owned())
                .collect::<Vec<_>>(),
        );
        let mode = selection.mode;
        hstack((
            toggle_group(
                [ToggleItem::text("Live"), ToggleItem::text("History")],
                mode,
            ),
            show(
                bind(move |runtime| RangeMode::from_index(mode.get(runtime)) == RangeMode::Live),
                move || {
                    toggle_group(
                        ChartWindow::ALL
                            .iter()
                            .map(|window| ToggleItem::text(window.label())),
                        selection.live_window,
                    )
                },
                move || select(options, selection.history),
            ),
            spacer(),
            export,
        ))
        .gap(spacing::ROOMY)
        .items_center()
        .w_full()
    })
}
