mod breakdown;
mod details;
mod summary;
mod swap;
mod table;
#[cfg(test)]
mod tests;

use crownui::prelude::{Cx, View};

use super::frame::{page_frame, panel};
use crate::app::AppState;
use crate::inspector;

pub fn page(cx: &mut Cx, state: AppState) -> impl View + use<> {
    let inspected = cx.signal(None);
    let strip = summary::summary_strip(cx, state);
    let breakdown = panel("Breakdown", breakdown::breakdown(cx, state));
    let swap = panel("Swap", swap::swap_panel(cx, state));
    let compression = panel("Compression", details::compression_panel(cx, state));
    let activity = panel("Activity", details::activity_facts(cx, state));
    let apps = panel("Apps", table::apps_panel(cx, state, inspected));
    let content = page_frame(
        cx,
        state,
        "Memory",
        (strip, breakdown, swap, compression, activity, apps),
    );
    inspector::wrap(cx, state, inspected, content)
}
