use std::net::{IpAddr, SocketAddr};

use monitor_core::{Bytes, BytesPerSecond, Pid, Uid};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Protocol {
    #[default]
    Tcp,
    Udp,
}

/// Who can reach a listening socket.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BindScope {
    /// `127.0.0.0/8` or `::1`: this machine only.
    Loopback,
    /// `0.0.0.0` or `::`: every interface, LAN-exposed.
    #[default]
    AllInterfaces,
    /// One specific address.
    Specific,
}

impl BindScope {
    pub fn of(address: IpAddr) -> Self {
        let address = match address {
            IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(address, IpAddr::V4),
            v4 => v4,
        };
        if address.is_loopback() {
            Self::Loopback
        } else if address.is_unspecified() {
            Self::AllInterfaces
        } else {
            Self::Specific
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TcpState {
    Established,
    SynSent,
    SynReceived,
    FinWait1,
    FinWait2,
    TimeWait,
    Close,
    CloseWait,
    LastAck,
    Listen,
    Closing,
    #[default]
    Unknown,
}

impl TcpState {
    pub(super) fn from_raw(raw: u8) -> Self {
        match raw {
            1 => Self::Established,
            2 => Self::SynSent,
            3 => Self::SynReceived,
            4 => Self::FinWait1,
            5 => Self::FinWait2,
            6 => Self::TimeWait,
            7 => Self::Close,
            8 => Self::CloseWait,
            9 => Self::LastAck,
            10 => Self::Listen,
            11 => Self::Closing,
            _ => Self::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListeningPort {
    pub protocol: Protocol,
    pub address: SocketAddr,
    pub scope: BindScope,
    /// Known only for processes whose descriptors could be read.
    pub owner: Option<Pid>,
    pub uid: Uid,
    pub inode: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Connection {
    pub protocol: Protocol,
    pub local: SocketAddr,
    pub remote: SocketAddr,
    /// UDP sockets report `Established` once connected.
    pub state: TcpState,
    pub receive_queue: u32,
    pub send_queue: u32,
    pub owner: Option<Pid>,
    pub uid: Uid,
    pub inode: u64,
}

/// TCP payload throughput of one process over the last traffic pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProcessTraffic {
    pub pid: Pid,
    pub receive: BytesPerSecond,
    pub transmit: BytesPerSecond,
    /// Bytes moved during that pass.
    pub received: Bytes,
    pub transmitted: Bytes,
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::*;

    #[test]
    fn classifies_bind_scopes() {
        assert_eq!(
            BindScope::of(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            BindScope::Loopback
        );
        assert_eq!(
            BindScope::of(IpAddr::V6(Ipv6Addr::LOCALHOST)),
            BindScope::Loopback
        );
        assert_eq!(
            BindScope::of(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
            BindScope::AllInterfaces
        );
        assert_eq!(
            BindScope::of(IpAddr::V6(Ipv6Addr::UNSPECIFIED)),
            BindScope::AllInterfaces
        );
        assert_eq!(
            BindScope::of(IpAddr::V6(Ipv4Addr::LOCALHOST.to_ipv6_mapped())),
            BindScope::Loopback
        );
        assert_eq!(
            BindScope::of(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 4))),
            BindScope::Specific
        );
    }
}
