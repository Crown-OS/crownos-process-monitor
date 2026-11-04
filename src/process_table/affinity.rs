use std::rc::Rc;

use crownui::kit::{
    ButtonOptions, ButtonVariant, ToggleOptions, button, dialog, toggle, use_theme,
};
use crownui::prelude::{
    AnyView, Cx, Listeners, Role, Semantic, Signal, Styled, View, bind, component, hstack,
    scroll_view, spacer, text, vstack,
};
use monitor_engine::system::{CpuMask, affinity};

use super::perform::RowAction;
use super::prompt::ActionPrompt;
use super::target::ActionTarget;
use crate::app::AppState;
use crate::components::app_table::RowId;
use crate::components::core_clusters::{CoreCluster, core_clusters};
use crate::util::spacing;

const DIALOG_WIDTH: f32 = 560.0;
const GRID_MAX_HEIGHT: f32 = 420.0;
const DIALOG_PADDING: f32 = 20.0;
const TITLE_SIZE: f32 = 16.0;
const GROUP_TITLE_SIZE: f32 = 11.0;
const CORE_COLUMNS: u16 = 4;
const GAP: f32 = 12.0;

/// One switch per logical CPU, by its position in the engine's CPU list.
struct Switches {
    cpus: Vec<usize>,
    on: Vec<Signal<bool>>,
}

impl Switches {
    fn chosen(&self, cx: &mut Cx) -> CpuMask {
        CpuMask::from_cpus(
            self.cpus
                .iter()
                .zip(&self.on)
                .filter(|(_, on)| on.get(cx))
                .map(|(cpu, _)| *cpu),
        )
    }

    fn fill(&self, cx: &mut Cx, mask: &CpuMask) {
        for (cpu, on) in self.cpus.iter().zip(&self.on) {
            on.set(cx, mask.contains(*cpu));
        }
    }
}

/// The CPU Affinity dialog of a process row: switches grouped like the CPU page's core map,
/// prefilled from the process's current affinity, applied off the UI thread.
#[derive(Clone, Copy)]
pub struct AffinityDialog {
    open: Signal<bool>,
    row: Signal<Option<RowId>>,
}

impl AffinityDialog {
    pub fn new(cx: &mut Cx) -> Self {
        Self {
            open: cx.signal(false),
            row: cx.signal(None),
        }
    }

    pub fn show(self, cx: &mut Cx, row: RowId) {
        self.row.set(cx, Some(row));
        self.open.set(cx, true);
    }

    pub fn view(self, state: AppState, prompt: ActionPrompt) -> impl View {
        let open = self.open;
        dialog(open, move || {
            component(move |cx| {
                let row = self.row.with_untracked(cx, |row| *row);
                let target = row.and_then(|row| {
                    state
                        .live
                        .with_untracked(cx, |live| ActionTarget::resolve(&live.apps, row))
                });
                let (clusters, cpus) = state.live.with_untracked(cx, |live| {
                    let logical = &live.tick.cpu.logical;
                    let cpus: Vec<usize> = logical
                        .iter()
                        .filter_map(|usage| usize::try_from(usage.cpu).ok())
                        .collect();
                    (core_clusters(&live.tick.cpu_info, cpus.len()), cpus)
                });
                let switches = Rc::new(Switches {
                    on: cpus.iter().map(|_| cx.signal(true)).collect(),
                    cpus,
                });
                let ready = cx.signal(false);
                if let Some(ActionTarget::Process { key, .. }) = target {
                    let read = async move {
                        tokio::task::spawn_blocking(move || affinity(key))
                            .await
                            .ok()
                            .and_then(Result::ok)
                    };
                    let filled = Rc::clone(&switches);
                    cx.spawn(read, move |cx, mask| {
                        if let Some(mask) = mask {
                            filled.fill(cx, &mask);
                        }
                        ready.set(cx, true);
                    });
                }
                let title = target.as_ref().map_or_else(
                    || "CPU Affinity".to_owned(),
                    |target| format!("CPU Affinity of {}", target.name()),
                );
                let groups: Vec<AnyView> = clusters
                    .into_iter()
                    .map(|cluster| cluster_switches(cx, cluster, &switches))
                    .collect();
                let applied = Rc::clone(&switches);
                let everything = Rc::clone(&switches);
                let counted = Rc::clone(&switches);
                let apply_blocked = bind(move |runtime| {
                    !ready.get(runtime) || counted.on.iter().all(|on| !on.get(runtime))
                });
                vstack((
                    text(title)
                        .text_size(TITLE_SIZE)
                        .font_semibold()
                        .role(Role::Heading),
                    scroll_view(vstack(groups).gap(GAP))
                        .max_h(GRID_MAX_HEIGHT)
                        .label("Logical CPUs"),
                    hstack((
                        button("All CPUs")
                            .variant(ButtonVariant::Secondary)
                            .on_click(move |cx, _| {
                                for on in &everything.on {
                                    on.set(cx, true);
                                }
                            }),
                        spacer(),
                        button("Cancel")
                            .variant(ButtonVariant::Secondary)
                            .on_click(move |cx, _| open.set(cx, false)),
                        button("Apply")
                            .variant(ButtonVariant::Primary)
                            .disabled(apply_blocked)
                            .on_click(move |cx, _| {
                                open.set(cx, false);
                                let chosen = applied.chosen(cx);
                                if let Some(row) = row {
                                    prompt.request(cx, row, RowAction::Affinity(chosen));
                                }
                            }),
                    ))
                    .gap(spacing::COMFORTABLE),
                ))
                .gap(GAP)
                .p(DIALOG_PADDING)
                .w(DIALOG_WIDTH)
                .label("CPU Affinity")
            })
        })
    }
}

fn cluster_switches(cx: &mut Cx, cluster: CoreCluster, switches: &Switches) -> AnyView {
    let theme = use_theme(cx);
    let cores: Vec<AnyView> = cluster
        .cells
        .into_iter()
        .map(|cell| {
            let siblings: Vec<AnyView> = cell
                .cpus
                .iter()
                .filter_map(|&position| {
                    let on = *switches.on.get(position)?;
                    let cpu = *switches.cpus.get(position)?;
                    Some(toggle(on).caption(format!("CPU {cpu}")).boxed())
                })
                .collect();
            vstack(siblings).gap(spacing::TIGHT).boxed()
        })
        .collect();
    vstack((
        text(cluster.label)
            .text_size(GROUP_TITLE_SIZE)
            .text_color(bind(move |runtime| {
                theme.with(runtime, |theme| theme.text.muted)
            })),
        vstack(cores)
            .grid()
            .grid_cols(CORE_COLUMNS)
            .gap(GAP)
            .w_full(),
    ))
    .gap(spacing::SNUG)
    .w_full()
    .boxed()
}
