//! The drawer's sections, in reading order (plan §1.8 without package provenance).

mod app_details;
mod environment;
mod executable;
mod family;
mod folded;
mod gpu;
mod identity;
pub mod network;
mod resources;
mod security;
mod startup;

use crownui::prelude::{AnyView, Signal};
use monitor_core::ProcessKey;
use monitor_engine::Snapshot;
use monitor_engine::system::inspect::ProcessDetails;

/// The fold states that survive the drawer's periodic refresh.
#[derive(Clone, Copy)]
pub struct Folds {
    pub desktop_file: Signal<bool>,
    pub environment: Signal<bool>,
}

pub fn all(
    details: &ProcessDetails,
    tick: &Snapshot,
    target: Signal<Option<ProcessKey>>,
    folds: Folds,
    utc_offset_seconds: i32,
) -> Vec<AnyView> {
    let pid = details.identity.key.pid;
    [
        identity::view(details, utc_offset_seconds),
        family::parent(details, &tick.processes, target),
        executable::view(details),
        security::view(details),
        startup::view(details),
        app_details::view(details, folds.desktop_file),
        resources::view(details),
        gpu::view(&tick.gpu, pid),
        network::view(&tick.network, pid),
        family::children(details, target),
        environment::view(details, folds.environment),
    ]
    .into_iter()
    .flatten()
    .collect()
}
