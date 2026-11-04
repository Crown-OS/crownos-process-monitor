use std::net::SocketAddr;

use monitor_core::{FdIndex, Uid};
use netlink_packet_core::NlasIterator;
use netlink_packet_sock_diag::constants::INET_DIAG_INFO;
use netlink_packet_sock_diag::inet::InetResponseHeader;

use super::model::{BindScope, Connection, ListeningPort, Protocol, TcpState};
use super::monitor::SocketNeeds;

/// `struct inet_diag_msg` is 72 bytes; attributes follow.
pub(super) const INET_RESPONSE_HEADER_LEN: usize = 72;
/// Offsets of `tcpi_bytes_acked` and `tcpi_bytes_received` in `struct tcp_info`.
const BYTES_ACKED: std::ops::Range<usize> = 120..128;
const BYTES_RECEIVED: std::ops::Range<usize> = 128..136;
const TCP_LISTEN: u8 = 10;
const UDP_UNCONNECTED: u8 = 7;
pub(super) const NO_INODE: u32 = 0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct TcpBytes {
    pub(super) acked: u64,
    pub(super) received: u64,
}

pub(super) fn record(
    header: &InetResponseHeader,
    protocol: Protocol,
    needs: SocketNeeds,
    fds: &FdIndex,
    listening: &mut Vec<ListeningPort>,
    connections: &mut Vec<Connection>,
) {
    let socket = &header.socket_id;
    let local = SocketAddr::new(socket.source_address, socket.source_port);
    let remote = SocketAddr::new(socket.destination_address, socket.destination_port);
    let inode = u64::from(header.inode);
    let owner = (header.inode != NO_INODE)
        .then(|| fds.socket_owner(inode))
        .flatten();
    let uid = Uid::new(header.uid);
    let is_listening = match protocol {
        Protocol::Tcp => header.state == TCP_LISTEN,
        Protocol::Udp => header.state == UDP_UNCONNECTED && remote.port() == 0 && local.port() != 0,
    };
    if is_listening {
        if needs.listening {
            listening.push(ListeningPort {
                protocol,
                address: local,
                scope: BindScope::of(local.ip()),
                owner,
                uid,
                inode,
            });
        }
    } else if needs.connections && remote.port() != 0 {
        connections.push(Connection {
            protocol,
            local,
            remote,
            state: TcpState::from_raw(header.state),
            receive_queue: header.recv_queue,
            send_queue: header.send_queue,
            owner,
            uid,
            inode,
        });
    }
}

pub(super) fn tcp_bytes(attributes: &[u8]) -> Option<TcpBytes> {
    let info = NlasIterator::new(attributes)
        .flatten()
        .find(|attribute| attribute.kind() == INET_DIAG_INFO)?;
    let value = info.value();
    let counter = |range: std::ops::Range<usize>| {
        value
            .get(range)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u64::from_ne_bytes)
    };
    Some(TcpBytes {
        acked: counter(BYTES_ACKED)?,
        received: counter(BYTES_RECEIVED)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_tcp_info_counters() {
        let mut info = vec![0_u8; 232];
        info[BYTES_ACKED].copy_from_slice(&4096_u64.to_ne_bytes());
        info[BYTES_RECEIVED].copy_from_slice(&1234_u64.to_ne_bytes());
        let mut attribute = Vec::new();
        attribute.extend_from_slice(&((4 + info.len()) as u16).to_ne_bytes());
        attribute.extend_from_slice(&INET_DIAG_INFO.to_ne_bytes());
        attribute.extend_from_slice(&info);
        assert_eq!(
            tcp_bytes(&attribute),
            Some(TcpBytes {
                acked: 4096,
                received: 1234
            })
        );
        assert_eq!(tcp_bytes(&attribute[..40]), None);
    }
}
