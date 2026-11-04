use crownui::prelude::{Cx, View, bind};

use crate::app::AppState;
use crate::components::stat_strip::{Figure, stat_strip};
use crate::util::format::bytes;

/// Used of total, available, free and swap in use.
pub fn summary_strip(_: &mut Cx, state: AppState) -> impl View + use<> {
    let live = state.live;
    let used = Figure::new(
        "Used",
        bind(move |runtime| live.with(runtime, |snapshot| bytes(snapshot.memory.used_bytes))),
    )
    .strain(bind(move |runtime| {
        live.with(runtime, |snapshot| snapshot.memory.strained)
    }));
    let total = Figure::new(
        "Of",
        bind(move |runtime| live.with(runtime, |snapshot| bytes(snapshot.memory.total_bytes))),
    );
    let available = Figure::new(
        "Available",
        bind(move |runtime| {
            live.with(runtime, |snapshot| {
                bytes(snapshot.tick.memory.available.get())
            })
        }),
    );
    let free = Figure::new(
        "Free",
        bind(move |runtime| live.with(runtime, |snapshot| bytes(snapshot.tick.memory.free.get()))),
    );
    let swap = Figure::new(
        "Swap used",
        bind(move |runtime| {
            live.with(runtime, |snapshot| {
                bytes(snapshot.tick.memory.swap_used.get())
            })
        }),
    );
    stat_strip(used, vec![total, available, free, swap])
}
