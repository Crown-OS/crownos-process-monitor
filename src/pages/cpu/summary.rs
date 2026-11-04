use crownui::prelude::{Cx, View, bind};

use crate::app::AppState;
use crate::app::live::latest;
use crate::components::stat_strip::{Figure, stat_strip};
use crate::util::format::percent;

/// Now, with its user and system shares.
pub fn summary_strip(_: &mut Cx, state: AppState) -> impl View + use<> {
    let live = state.live;
    let now = Figure::new(
        "CPU",
        bind(move |runtime| live.with(runtime, |snapshot| percent(latest(&snapshot.cpu.total)))),
    )
    .strain(bind(move |runtime| {
        live.with(runtime, |snapshot| snapshot.cpu.busy_strained)
    }));
    let user = Figure::new(
        "User",
        bind(move |runtime| live.with(runtime, |snapshot| percent(latest(&snapshot.cpu.user)))),
    );
    let system = Figure::new(
        "System",
        bind(move |runtime| live.with(runtime, |snapshot| percent(latest(&snapshot.cpu.system)))),
    );
    stat_strip(now, vec![user, system])
}
