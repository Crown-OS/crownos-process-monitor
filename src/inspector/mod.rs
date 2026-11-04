//! The Process Inspector: a drawer beside the page with everything known about one process,
//! re-read off the UI thread every few seconds while it is open.

mod export;
mod fields;
mod load;
mod sections;
mod series;

use crownui::kit::{Toasts, provide_toasts, toast_host, use_theme};
use crownui::prelude::{
    Cx, Memo, Runtime, Signal, Styled, View, bind, component, keyed, text, vstack,
};
use monitor_core::ProcessKey;
use monitor_engine::utc_offset_seconds;

pub use self::load::{InspectorData, Loaded};
pub use self::sections::network::{protocol_name, scope_label, tcp_state_name};

use self::export::{copy_as_text, export_buttons};
use self::sections::Folds;
use self::series::ProcessSeries;
use crate::app::AppState;
use crate::components::app_table::RowId;
use crate::components::chart::{YRange, area_chart};
use crate::components::drawer::drawer;
use crate::process_table::representative;
use crate::util::format::{bytes, percent};
use crate::util::spacing;
use crate::util::theme::Subsystem;

const CHART_HEIGHT: f32 = 64.0;
const CHART_LABEL_SIZE: f32 = 12.0;

/// What one drawer shows and remembers across refreshes.
#[derive(Clone, Copy)]
pub struct Inspector {
    pub target: Signal<Option<ProcessKey>>,
    pub data: Signal<InspectorData>,
    series: Signal<ProcessSeries>,
    folds: Folds,
}

impl Inspector {
    pub fn new(cx: &mut Cx) -> Self {
        Self {
            target: cx.signal(None),
            data: cx.signal(InspectorData::default()),
            series: cx.signal(ProcessSeries::default()),
            folds: Folds {
                desktop_file: cx.signal(false),
                environment: cx.signal(false),
            },
        }
    }
}

/// The drawer over a table's inspected row: an app row shows its main process.
pub fn wrap<V: View>(
    cx: &mut Cx,
    state: AppState,
    inspected: Signal<Option<RowId>>,
    content: V,
) -> impl View + use<V> {
    let inspector = Inspector::new(cx);
    follow_rows(cx, state, inspector, inspected);
    inspector_drawer(cx, state, inspector, content)
}

/// Opens `inspector` on the process behind each row written to `inspected`, and clears
/// `inspected` when the drawer closes, so inspecting the same row again reopens it.
pub fn follow_rows(
    cx: &mut Cx,
    state: AppState,
    inspector: Inspector,
    inspected: Signal<Option<RowId>>,
) {
    let target = inspector.target;
    cx.effect(move |runtime| {
        let Some(row) = inspected.get(runtime) else {
            return;
        };
        let key = state.live.with_untracked(runtime, |live| {
            representative(&live.apps, row).map(|process| process.facts.key(process.pid))
        });
        if key.is_some() && target.with_untracked(runtime, |current| *current != key) {
            target.set(runtime, key);
        }
    });
    cx.effect(move |runtime| {
        if target.get(runtime).is_none() && inspected.with_untracked(runtime, Option::is_some) {
            inspected.set(runtime, None);
        }
    });
}

/// The drawer over any process: set `inspector.target` to open it, e.g. from a socket row.
pub fn inspector_drawer<V: View>(
    cx: &mut Cx,
    state: AppState,
    inspector: Inspector,
    content: V,
) -> impl View + use<V> {
    let target = inspector.target;
    let open = cx.signal(false);
    cx.effect(move |runtime| {
        let shown = target.get(runtime).is_some();
        if open.with_untracked(runtime, |open| *open != shown) {
            open.set(runtime, shown);
        }
    });
    cx.effect(move |runtime| {
        if !open.get(runtime) && target.with_untracked(runtime, Option::is_some) {
            target.set(runtime, None);
        }
    });
    cx.effect(move |runtime| {
        let key = target.get(runtime);
        state.set_inspected(runtime, key);
    });
    cx.on_cleanup(move |runtime| state.set_inspected(runtime, None));
    let title = title_memo(cx, state, inspector);
    component(move |cx| {
        let toasts = provide_toasts(cx);
        let data = inspector.data;
        toast_host(drawer(
            open,
            title,
            move || sheet(state, inspector, toasts),
            move |cx: &mut Cx| copy_as_text(cx, data, toasts),
            content,
        ))
        .w_full()
        .h_full()
    })
}

fn title_memo(cx: &mut Cx, state: AppState, inspector: Inspector) -> Memo<String> {
    let (target, data) = (inspector.target, inspector.data);
    cx.memo(move |runtime| {
        let Some(key) = target.get(runtime) else {
            return String::new();
        };
        let loaded = data.with(runtime, |data| {
            data.details()
                .filter(|details| details.identity.key == key)
                .map(|details| details.identity.name.clone())
        });
        loaded.unwrap_or_else(|| {
            state.live.with_untracked(runtime, |live| {
                live.tick.processes.process(key).map_or_else(
                    || format!("PID {}", key.pid.get()),
                    |row| row.name.to_string(),
                )
            })
        })
    })
}

fn sheet(state: AppState, inspector: Inspector, toasts: Toasts) -> impl View {
    component(move |cx| {
        series::follow(cx, state, inspector.target, inspector.series);
        let (target, data) = (inspector.target, inspector.data);
        let loader = keyed(
            move |runtime| target.get(runtime),
            move |cx, key| {
                if let Some(key) = key {
                    restart_if_retargeted(cx, data, key);
                    load::follow(cx, key, data);
                }
                vstack(())
            },
        );
        let body = keyed(
            move |runtime| data.with(runtime, |data| data.revision),
            move |cx, _| body(cx, state, inspector),
        );
        vstack((
            live_charts(cx, state, inspector.series),
            body,
            export_buttons(data, toasts),
            loader,
        ))
        .gap(spacing::SPACIOUS)
        .w_full()
    })
}

fn restart_if_retargeted(runtime: &mut Runtime, data: Signal<InspectorData>, key: ProcessKey) {
    let stale = data.with_untracked(runtime, |data| {
        data.details()
            .is_none_or(|details| details.identity.key != key)
    });
    if stale {
        data.update(runtime, |data| {
            data.revision += 1;
            data.loaded = Loaded::Loading;
        });
    }
}

fn body(cx: &mut Cx, state: AppState, inspector: Inspector) -> impl View + use<> {
    let loaded = inspector
        .data
        .with_untracked(cx, |data| data.loaded.clone());
    let views = match loaded {
        Loaded::Ready(details) => state.live.with_untracked(cx, |live| {
            sections::all(
                &details,
                &live.tick,
                inspector.target,
                inspector.folds,
                utc_offset_seconds(),
            )
        }),
        Loaded::Loading => vec![notice("Reading the process…").boxed()],
        Loaded::Ended => vec![notice("This process has ended.").boxed()],
        Loaded::Failed(reason) => {
            vec![notice(format!("Cannot read this process: {reason}")).boxed()]
        }
    };
    vstack(views).gap(spacing::SPACIOUS).w_full()
}

fn notice(message: impl Into<String>) -> impl View {
    let message = message.into();
    component(move |cx| {
        let theme = use_theme(cx);
        text(message.clone())
            .text_size(CHART_LABEL_SIZE)
            .text_color(bind(move |runtime| {
                theme.with(runtime, |theme| theme.text.muted)
            }))
    })
}

fn live_charts(cx: &mut Cx, state: AppState, series: Signal<ProcessSeries>) -> impl View + use<> {
    let cpu_tint = state.tint(cx, Subsystem::Cpu);
    let memory_tint = state.tint(cx, Subsystem::Memory);
    let latest = move |pick: fn(&ProcessSeries) -> &[f32]| {
        move |runtime: &mut Runtime| {
            series.with(runtime, |series| {
                pick(series).last().copied().unwrap_or(f32::NAN)
            })
        }
    };
    let cpu_now = latest(|series| &series.cpu);
    let memory_now = latest(|series| &series.memory);
    vstack((
        text(bind(move |runtime| {
            format!("CPU {}", percent(cpu_now(runtime)))
        }))
        .text_size(CHART_LABEL_SIZE),
        area_chart(
            move |runtime: &mut Runtime, sink: &mut dyn FnMut(&[f32])| {
                series.with(runtime, |series| sink(&series.cpu));
            },
            cpu_tint,
        )
        .range(YRange::AUTO)
        .height(CHART_HEIGHT),
        text(bind(move |runtime| {
            let resident = memory_now(runtime);
            let shown = if resident.is_finite() {
                bytes(resident as u64)
            } else {
                "–".to_owned()
            };
            format!("Memory {shown}")
        }))
        .text_size(CHART_LABEL_SIZE),
        area_chart(
            move |runtime: &mut Runtime, sink: &mut dyn FnMut(&[f32])| {
                series.with(runtime, |series| sink(&series.memory));
            },
            memory_tint,
        )
        .range(YRange::AUTO)
        .height(CHART_HEIGHT),
    ))
    .gap(spacing::SNUG)
    .w_full()
}

#[cfg(test)]
mod tests;
