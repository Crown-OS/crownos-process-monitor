mod activity;
mod charts;
mod fan_control;
mod hardware;
mod sensors;
mod summary;
mod table;
#[cfg(test)]
mod tests;

use crownui::kit::{provide_toasts, toast_host};
use crownui::prelude::{Cx, Styled, View};

use super::frame::{page_frame, panel};
use crate::app::AppState;
use crate::inspector;

pub fn page(cx: &mut Cx, state: AppState) -> impl View + use<> {
    let inspected = cx.signal(None);
    let strip = summary::summary_strip(cx, state);
    let hardware = panel("Hardware", hardware::hardware_panel(cx, state));
    let utilization = panel("Utilization", charts::utilization(cx, state));
    let apps = panel("Apps", table::apps_panel(cx, state, inspected));
    let activity = panel("Load and activity", activity::activity_cards(cx, state));
    let scaling = panel(
        "Frequency and scaling",
        (
            charts::frequency_chart(cx, state),
            activity::scaling_facts(cx, state),
        ),
    );
    let toasts = provide_toasts(cx);
    let sensors = panel("Sensors and fans", sensors::sensors_panel(state, toasts));
    let content = page_frame(
        cx,
        state,
        "CPU",
        (
            strip,
            hardware,
            utilization,
            apps,
            activity,
            scaling,
            sensors,
        ),
    );
    let content = toast_host(content).w_full().h_full();
    inspector::wrap(cx, state, inspected, content)
}
