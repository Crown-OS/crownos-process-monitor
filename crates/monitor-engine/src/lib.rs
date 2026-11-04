//! The engine of the CrownOS process monitor: one sampler thread that owns
//! every collector, samples at the cadence the UI's [`Interest`](monitor_core::Interest) asks for,
//! joins per-process GPU and network figures into the process table, feeds
//! history and alerts, and publishes one [`Snapshot`] per tick.
//!
//! The engine lives inside the app process and stops with it (decision D3):
//! dropping the [`EngineHandle`] flushes history and joins the thread.
//!
//! When the optional privileged helper is installed (decision D2), the
//! engine also fills in what an unprivileged process cannot see: other
//! users' I/O and PSS, every socket and DRM client, RAPL energy and per-app
//! traffic of every protocol. Its state is [`Snapshot::helper`]; writes go
//! through [`Command::HelperSignal`], [`Command::HelperRenice`],
//! [`Command::SetFan`] and [`Command::RestoreFans`], and their outcomes
//! arrive on [`EngineControl::helper_outcomes`].
//!
//! Drive actions ([`Command::Drive`]) run through the sampler's UDisks2
//! handle and report on [`EngineControl::drive_outcomes`]; inspector details
//! come from [`EngineControl::inspect`], which reuses the sampler's desktop
//! index and app grouping.

mod alerting;
mod availability;
mod command;
mod config;
mod drives;
mod energy;
mod error;
mod handle;
mod notify;
mod privileged;
mod recording;
mod sampler;
mod snapshot;
mod util;

pub use monitor_alerts as alerts;
pub use monitor_hardware as hardware;
pub use monitor_helper as helper;
pub use monitor_history as history;
pub use monitor_io as io;
pub use monitor_system as system;

pub use availability::{Availability, Domain, DomainCosts};
pub use command::Command;
pub use config::{EngineConfig, EngineSettings, Notifications};
pub use drives::{DriveAction, DriveOutcome};
pub use energy::{AppEnergy, EnergyEstimate, EnergySource};
pub use error::EngineError;
pub use handle::{Engine, EngineControl, EngineHandle};
pub use privileged::{HelperAction, HelperConnection, HelperFailure, HelperOutcome, HelperStatus};
pub use recording::history_engine;
pub use snapshot::Snapshot;
pub use util::local_time::utc_offset_seconds;
