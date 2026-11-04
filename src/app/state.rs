use crownui::kit::{Theme, use_theme};
use crownui::prelude::{Color, Cx, Memo, Runtime, Signal};
use monitor_core::ProcessKey;

use super::live::LiveSnapshot;
use crate::process_table::AlertRequest;
use crate::settings::{ChartWindow, Settings};
use crate::util::theme::{Palette, Subsystem, series_tint, subsystem_tint};

/// What every page is handed: the settings, the live readings and the theme.
#[derive(Clone, Copy)]
pub struct AppState {
    pub settings: Settings,
    pub live: Signal<LiveSnapshot>,
    pub theme: Signal<Theme>,
    pub palette: Memo<Palette>,
    pub chart_window: Memo<ChartWindow>,
    /// The process an inspector drawer shows; the engine reads its extras while set.
    pub inspected: Signal<Option<ProcessKey>>,
    /// The latest "Alert when…" pick from a table's context menu, for the alerts owner to
    /// turn into a rule.
    pub alert_request: Signal<Option<AlertRequest>>,
}

impl AppState {
    pub fn new(cx: &mut Cx, settings: Settings, live: LiveSnapshot) -> Self {
        Self {
            settings,
            live: cx.signal(live),
            theme: use_theme(cx),
            palette: settings.field(cx, |settings| settings.palette),
            chart_window: settings.field(cx, |settings| settings.chart_window),
            inspected: cx.signal(None),
            alert_request: cx.signal(None),
        }
    }

    /// Points the engine's per-process extras at `key`; the bridge forwards it as interest.
    pub fn set_inspected(self, runtime: &mut Runtime, key: Option<ProcessKey>) {
        if self
            .inspected
            .with_untracked(runtime, |current| *current != key)
        {
            self.inspected.set(runtime, key);
        }
    }

    pub fn tint(self, cx: &mut Cx, subsystem: Subsystem) -> Memo<Color> {
        subsystem_tint(cx, self.theme, self.palette, subsystem)
    }

    pub fn series(self, cx: &mut Cx, index: usize) -> Memo<Color> {
        series_tint(cx, self.theme, self.palette, index)
    }
}
