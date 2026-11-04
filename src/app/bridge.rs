//! Feeds the live snapshot from the engine: one UI update per tick, the
//! page and window state sent back as the engine's interest.

mod apps;
pub(super) mod cpu;
mod cpu_today;
mod daily;
pub(super) mod disk;
#[cfg(test)]
pub(crate) mod fixture;
pub(super) mod gpu;
pub(super) mod memory;
pub(super) mod network;
pub(super) mod overview;
mod overview_insights;
pub(super) mod process_detail;
mod readings;
pub(super) mod row_facts;
mod settings;

use std::sync::Arc;

use crownui::prelude::{Cx, Signal};
use monitor_core::Interest;
use monitor_engine::{EngineControl, Snapshot};
use tokio::sync::watch;

use self::apps::AppListFiller;
use super::live::LiveSnapshot;
use super::navigation::{Navigation, Page};
use super::state::AppState;

pub use self::settings::engine_settings;

/// Starts pumping engine snapshots into `state.live` for as long as the
/// calling component lives.
pub fn start(cx: &mut Cx, state: AppState, navigation: Navigation, engine: EngineControl) {
    super::EngineLink::new(engine.clone()).provide(cx);
    state.live.update(cx, readings::prepare);
    follow_page(cx, navigation, state, engine.clone());
    follow_settings(cx, state, engine.clone());
    daily::refresh(cx, state.live, engine.clone());
    cpu_today::refresh(cx, state.live, engine.clone());
    let feed = Feed {
        live: state.live,
        settings: state.settings,
        apps: AppListFiller::new(),
    };
    pump(cx, engine.snapshots(), feed);
}

struct Feed {
    live: Signal<LiveSnapshot>,
    settings: crate::settings::Settings,
    apps: AppListFiller,
}

impl Feed {
    fn apply(&mut self, cx: &mut Cx, snapshot: &Arc<Snapshot>) {
        let cpu_scale = self.settings.current(cx).cpu_scale;
        let apps = &mut self.apps;
        self.live.update(cx, |live| {
            readings::append(live, snapshot);
            apps.refill(&mut live.apps, &snapshot.processes, cpu_scale);
            live.details.refill(&live.apps, snapshot);
            live.tick = Arc::clone(snapshot);
        });
    }
}

/// The worker pattern over a watch channel: await the next tick off the UI
/// thread, apply it on the UI thread, re-arm. Ticks the UI was too busy for
/// are skipped, never queued.
fn pump(cx: &mut Cx, mut snapshots: watch::Receiver<Arc<Snapshot>>, mut feed: Feed) {
    let next_tick = async move {
        let open = snapshots.changed().await.is_ok();
        let snapshot = Arc::clone(&snapshots.borrow_and_update());
        (snapshots, snapshot, open)
    };
    cx.spawn(next_tick, move |cx, (snapshots, snapshot, open)| {
        if open {
            feed.apply(cx, &snapshot);
            pump(cx, snapshots, feed);
        }
    });
}

fn follow_page(cx: &mut Cx, navigation: Navigation, state: AppState, engine: EngineControl) {
    let (live, inspected) = (state.live, state.inspected);
    let current = navigation.current();
    let selected_gpu =
        cx.memo(move |runtime| live.with(runtime, |snapshot| snapshot.gpu.selected.clone()));
    cx.effect(move |runtime| {
        let interest = Interest {
            page: engine_page(Page::from_index(current.get(runtime))),
            selected_gpu: selected_gpu.get(runtime),
            inspected: inspected.get(runtime),
            ..Interest::default()
        };
        let _ = engine.set_interest(interest);
    });
}

fn follow_settings(cx: &mut Cx, state: AppState, engine: EngineControl) {
    overview_insights::follow(cx, state, engine);
}

const fn engine_page(page: Page) -> monitor_core::Page {
    match page {
        Page::Overview => monitor_core::Page::Overview,
        Page::Cpu => monitor_core::Page::Cpu,
        Page::Gpu => monitor_core::Page::Gpu,
        Page::Memory => monitor_core::Page::Memory,
        Page::Disk => monitor_core::Page::Disk,
        Page::Network => monitor_core::Page::Network,
    }
}
