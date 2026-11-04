//! TCP and UDP sockets through `NETLINK_SOCK_DIAG`: listening ports,
//! connections and per-process TCP traffic from `tcp_info` byte counters.

mod model;
mod monitor;
mod parse;

pub use model::{BindScope, Connection, ListeningPort, ProcessTraffic, Protocol, TcpState};
pub(crate) use monitor::{SocketMonitor, SocketNeeds, SocketOutputs};
