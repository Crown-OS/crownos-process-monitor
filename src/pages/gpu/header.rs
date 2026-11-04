use crownui::kit::{CalloutOptions, ToggleItem, Tone, callout, toggle_group};
use crownui::prelude::{Cx, Styled, View, bind, show, vstack};
use monitor_engine::hardware::{GpuDevice, GpuPower};

use super::selection::GpuChoice;
use crate::app::AppState;
use crate::util::spacing;

pub const ASLEEP_NOTE: &str =
    "This GPU is runtime-suspended. Readings resume when an app wakes it; the monitor never does.";

fn is_asleep(device: Option<&GpuDevice>) -> bool {
    device.is_some_and(|device| device.metrics.power_state == GpuPower::Asleep)
}

/// The segmented selector when there is more than one GPU, then the chosen GPU's asleep notice.
pub fn header(
    _: &mut Cx,
    state: AppState,
    choice: GpuChoice,
    names: Vec<String>,
) -> impl View + use<> {
    let live = state.live;
    let selector = (names.len() > 1)
        .then(|| toggle_group(names.into_iter().map(ToggleItem::text), choice.index));
    let asleep_note = show(
        bind(move |runtime| choice.with(runtime, live, is_asleep)),
        || callout(Tone::Neutral, "Asleep").body(ASLEEP_NOTE),
        || vstack(()),
    );
    vstack((selector, asleep_note)).gap(spacing::ROOMY).w_full()
}
