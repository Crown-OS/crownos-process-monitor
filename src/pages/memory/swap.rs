use crownui::prelude::{
    Cx, Runtime, Semantic, Styled, View, bind, component, keyed, show, text, vstack,
};
use monitor_engine::system::{SwapDevice, SwapKind};

use super::super::sources::{names_key, percent_readout, windowed};
use crate::app::AppState;
use crate::app::live::LiveSnapshot;
use crate::components::chart::{YRange, area_chart};
use crate::components::fact_grid::{Fact, fact_grid};
use crate::util::format::{bytes, count};
use crate::util::spacing;
use crate::util::theme::Subsystem;

const CHART_HEIGHT: f32 = 110.0;
const NOTE_SIZE: f32 = 12.0;
pub const NO_SWAP: &str = "No swap is configured.";

const fn kind_label(kind: SwapKind) -> &'static str {
    match kind {
        SwapKind::Partition => "partition",
        SwapKind::File => "file",
        SwapKind::Zram => "zram",
    }
}

/// "zram · 1.00 GB of 8.00 GB · priority 100".
pub fn describe_device(device: &SwapDevice) -> String {
    format!(
        "{} · {} of {} · priority {}",
        kind_label(device.kind),
        bytes(device.used.get()),
        bytes(device.size.get()),
        device.priority
    )
}

fn device_names(snapshot: &LiveSnapshot) -> u64 {
    names_key(
        snapshot
            .tick
            .memory
            .swap_devices
            .iter()
            .map(|device| &*device.name),
    )
}

fn devices(cx: &mut Cx, state: AppState) -> impl View + use<> {
    let live = state.live;
    let names: Vec<String> = live.with_untracked(cx, |snapshot| {
        snapshot
            .tick
            .memory
            .swap_devices
            .iter()
            .map(|device| device.name.to_string())
            .collect()
    });
    let facts = names
        .into_iter()
        .enumerate()
        .map(|(index, name)| {
            Fact::new(
                name,
                bind(move |runtime: &mut Runtime| {
                    live.with(runtime, |snapshot| {
                        snapshot
                            .tick
                            .memory
                            .swap_devices
                            .get(index)
                            .map_or_else(String::new, describe_device)
                    })
                }),
            )
        })
        .chain(std::iter::once(Fact::new(
            "Swap in · out",
            bind(move |runtime: &mut Runtime| {
                live.with(runtime, |snapshot| {
                    let memory = &snapshot.tick.memory;
                    format!(
                        "{} · {} pages/s",
                        count(memory.swap_in_rate.max(0.0).round() as u64),
                        count(memory.swap_out_rate.max(0.0).round() as u64)
                    )
                })
            }),
        )))
        .collect();
    fact_grid(facts)
}

fn swap_view(cx: &mut Cx, state: AppState) -> impl View + use<> {
    let live = state.live;
    let chart = area_chart(
        windowed(state, |snapshot| &snapshot.memory.swap_percent),
        state.tint(cx, Subsystem::Memory),
    )
    .range(YRange::PERCENT)
    .readout(percent_readout)
    .height(CHART_HEIGHT);
    let list = keyed(
        move |runtime| live.with(runtime, device_names),
        move |cx, _| devices(cx, state),
    );
    vstack((chart, list))
        .gap(spacing::LOOSE)
        .w_full()
        .label("Swap used")
}

/// The swap chart and devices once any swap exists; a note otherwise.
pub fn swap_panel(_: &mut Cx, state: AppState) -> impl View + use<> {
    let live = state.live;
    show(
        bind(move |runtime| {
            live.with(runtime, |snapshot| {
                snapshot.tick.memory.swap_total.get() > 0
            })
        }),
        move || component(move |cx| swap_view(cx, state)),
        || text(NO_SWAP).text_size(NOTE_SIZE),
    )
}

#[cfg(test)]
mod tests {
    use monitor_core::Bytes;

    use super::*;

    #[test]
    fn devices_read_kind_usage_and_priority() {
        let device = SwapDevice {
            name: "/dev/zram0".into(),
            kind: SwapKind::Zram,
            size: Bytes::new(8 << 30),
            used: Bytes::new(1 << 30),
            priority: 100,
        };
        assert_eq!(
            describe_device(&device),
            "zram · 1.00 GB of 8.00 GB · priority 100"
        );
    }
}
