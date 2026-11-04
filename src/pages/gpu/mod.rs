mod accelerators;
mod charts;
mod describe;
mod details;
mod header;
mod selection;
mod table;
#[cfg(test)]
mod tests;

use crownui::kit::icons;
use crownui::prelude::{AnyView, Cx, Styled, View, bind, component, keyed, show, vstack};

use super::frame::{page_frame, panel};
use super::sources::names_key;
use crate::app::AppState;
use crate::app::live::LiveSnapshot;
use crate::components::empty_state::{NOT_AVAILABLE, empty_state};
use crate::inspector;
use selection::GpuChoice;

fn device_keys(snapshot: &LiveSnapshot) -> u64 {
    names_key(
        snapshot
            .tick
            .gpu
            .devices
            .iter()
            .map(|device| device.identity.key.as_str()),
    )
}

fn device_names(snapshot: &LiveSnapshot) -> Vec<String> {
    snapshot
        .tick
        .gpu
        .devices
        .iter()
        .map(|device| device.identity.name.to_string())
        .collect()
}

fn no_gpu(cx: &mut Cx, state: AppState) -> AnyView {
    let reason = state.live.with_untracked(cx, |snapshot| {
        snapshot
            .tick
            .availability
            .failure(monitor_engine::Domain::Gpu)
            .map_or_else(
                || "No GPU was found under /sys/class/drm.".to_owned(),
                str::to_owned,
            )
    });
    empty_state(icons::MONITOR, NOT_AVAILABLE, reason).boxed()
}

/// Everything about the chosen GPU, rebuilt only when the set of GPUs changes.
fn devices(cx: &mut Cx, state: AppState) -> AnyView {
    let names = state.live.with_untracked(cx, device_names);
    if names.is_empty() {
        return no_gpu(cx, state);
    }
    let choice = GpuChoice::new(cx, state.live);
    vstack((
        header::header(cx, state, choice, names),
        panel("Hardware", details::hardware_facts(cx, state, choice)),
        panel("Utilization", charts::utilization(cx, state, choice)),
        panel("Readings", details::live_facts(cx, state, choice)),
    ))
    .gap(state.settings.current(cx).spacing.gap())
    .w_full()
    .boxed()
}

pub fn page(cx: &mut Cx, state: AppState) -> impl View + use<> {
    let live = state.live;
    let inspected = cx.signal(None);
    let body = keyed(
        move |runtime| live.with(runtime, device_keys),
        move |cx, _| devices(cx, state),
    );
    let accelerators = show(
        bind(move |runtime| {
            live.with(runtime, |snapshot| {
                !snapshot.tick.gpu.accelerators.is_empty()
            })
        }),
        move || {
            panel(
                "Accelerators",
                component(move |cx| accelerators::accelerator_list(cx, state)),
            )
        },
        || vstack(()),
    );
    let apps = panel("Apps", table::apps_panel(cx, state, inspected));
    let content = page_frame(cx, state, "GPU", (body, accelerators, apps));
    inspector::wrap(cx, state, inspected, content)
}
