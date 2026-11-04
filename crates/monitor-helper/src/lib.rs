//! The optional privileged helper of the CrownOS Process Monitor: a system
//! D-Bus service that answers what an unprivileged monitor cannot measure,
//! behind a replaceable authorization backend, plus [`HelperClient`] for
//! the engine.
//!
//! Installation: the binary goes to `/usr/lib/crownos/monitor-helper`;
//! `data/org.crownos.ProcessMonitor1.conf` to `/usr/share/dbus-1/system.d/`,
//! `data/org.crownos.ProcessMonitor1.service` to
//! `/usr/share/dbus-1/system-services/`, `data/crownos-monitor-helper.service`
//! to `/usr/lib/systemd/system/` and `data/org.crownos.process-monitor.policy`
//! to `/usr/share/polkit-1/actions/`. Builds without the `polkit` feature read
//! `/etc/crownos/process-monitor-helper.rules` instead (see
//! `data/process-monitor-helper.rules.example`). Per-cgroup network
//! accounting needs `--features ebpf`, which compiles
//! `src/bpf/cgroup_traffic.bpf.c` with clang.

mod actions;
pub mod auth;
#[cfg(feature = "ebpf")]
mod bpf;
mod caller;
mod capability;
mod client;
mod error;
mod fans;
pub mod lifetime;
mod protocol;
mod service;
mod sources;
mod util;

pub use caller::Caller;
pub use capability::Capability;
pub use client::{ActionReceiver, HelperAvailability, HelperClient};
pub use error::HelperError;
pub use fans::fan_id_string;
pub use protocol::{
    API_VERSION, BUS_NAME, CgroupTraffic, DrmClient, EnergyZone, INTERFACE_NAME, OBJECT_PATH,
    ProcessUsage, SignalKind, SocketOwner,
};
pub use service::{
    HelperState, Method, ProcessMonitor, capability_for, serve, supported_capabilities,
};
