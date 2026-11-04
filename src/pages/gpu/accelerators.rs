use crownui::prelude::{Cx, Runtime, View, bind, keyed};
use monitor_engine::hardware::{AcceleratorSnapshot, GpuPower};

use super::super::sources::names_key;
use crate::app::AppState;
use crate::components::fact_grid::{Fact, fact_grid};
use crate::util::format::percent;

/// "amdxdna · 12% busy", or "intel_vpu · asleep".
pub fn describe_accelerator(accelerator: &AcceleratorSnapshot) -> String {
    let activity = match (accelerator.power_state, accelerator.busy) {
        (GpuPower::Asleep, _) => "asleep".to_owned(),
        (_, Some(busy)) => format!("{} busy", percent(busy.get())),
        (_, None) => "busy time not exposed".to_owned(),
    };
    format!("{} · {activity}", accelerator.driver)
}

/// One row per NPU; the caller shows the section only when one exists.
pub fn accelerator_list(_: &mut Cx, state: AppState) -> impl View + use<> {
    let live = state.live;
    keyed(
        move |runtime| {
            live.with(runtime, |snapshot| {
                names_key(
                    snapshot
                        .tick
                        .gpu
                        .accelerators
                        .iter()
                        .map(|accelerator| accelerator.key.as_str()),
                )
            })
        },
        move |cx, _| {
            let names: Vec<String> = live.with_untracked(cx, |snapshot| {
                snapshot
                    .tick
                    .gpu
                    .accelerators
                    .iter()
                    .map(|accelerator| accelerator.name.to_string())
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
                                    .gpu
                                    .accelerators
                                    .get(index)
                                    .map_or_else(String::new, describe_accelerator)
                            })
                        }),
                    )
                })
                .collect();
            fact_grid(facts)
        },
    )
}
