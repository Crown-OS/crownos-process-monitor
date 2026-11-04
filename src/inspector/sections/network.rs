use crownui::prelude::AnyView;
use monitor_core::Pid;
use monitor_engine::io::{BindScope, NetworkSnapshot, Protocol, TcpState};

use crate::inspector::fields::{Fields, section};

pub const fn protocol_name(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::Tcp => "TCP",
        Protocol::Udp => "UDP",
    }
}

/// Who can reach a listening socket, as the badge reads.
pub const fn scope_label(scope: BindScope) -> &'static str {
    match scope {
        BindScope::Loopback => "Loopback only",
        BindScope::AllInterfaces => "Exposed on all interfaces",
        BindScope::Specific => "Exposed on one address",
    }
}

pub const fn tcp_state_name(state: TcpState) -> &'static str {
    match state {
        TcpState::Established => "Established",
        TcpState::SynSent => "Connecting",
        TcpState::SynReceived => "Accepting",
        TcpState::FinWait1 | TcpState::FinWait2 => "Closing",
        TcpState::TimeWait => "Time wait",
        TcpState::Close => "Closed",
        TcpState::CloseWait => "Close wait",
        TcpState::LastAck => "Last ack",
        TcpState::Listen => "Listening",
        TcpState::Closing => "Closing",
        TcpState::Unknown => "",
    }
}

/// The process's listening ports and open connections.
pub fn view(network: &NetworkSnapshot, pid: Pid) -> Option<AnyView> {
    let mut fields = Fields::default();
    for port in network
        .listening
        .iter()
        .filter(|port| port.owner == Some(pid))
    {
        fields.add(
            "Listening",
            format!(
                "{} {} · {}",
                protocol_name(port.protocol),
                port.address,
                scope_label(port.scope)
            ),
        );
    }
    for connection in network
        .connections
        .iter()
        .filter(|connection| connection.owner == Some(pid))
    {
        fields.add(
            "Connection",
            format!(
                "{} {} → {} {}",
                protocol_name(connection.protocol),
                connection.local,
                connection.remote,
                tcp_state_name(connection.state)
            ),
        );
    }
    (!fields.is_empty()).then(|| section("Network", fields.into_views()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_scopes_say_who_can_connect() {
        assert_eq!(scope_label(BindScope::Loopback), "Loopback only");
        assert_eq!(
            scope_label(BindScope::of(std::net::Ipv4Addr::UNSPECIFIED.into())),
            "Exposed on all interfaces"
        );
        assert_eq!(
            scope_label(BindScope::of(std::net::Ipv6Addr::LOCALHOST.into())),
            "Loopback only"
        );
        assert_eq!(
            scope_label(BindScope::of(
                std::net::Ipv4Addr::new(192, 168, 1, 4).into()
            )),
            "Exposed on one address"
        );
    }
}
