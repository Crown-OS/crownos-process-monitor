use std::collections::HashMap;
use std::io;

use monitor_core::{Bytes, BytesPerSecond, FdIndex, Pid};
use netlink_packet_core::Parseable;
use netlink_packet_sock_diag::SockDiagMessage;
use netlink_packet_sock_diag::constants::{
    AF_INET, AF_INET6, IPPROTO_TCP, IPPROTO_UDP, SOCK_DIAG_BY_FAMILY,
};
use netlink_packet_sock_diag::inet::{
    ExtensionFlags, InetRequest, InetResponseHeader, SocketId, StateFlags,
};
use netlink_sys::protocols::NETLINK_SOCK_DIAG;

use super::model::{Connection, ListeningPort, ProcessTraffic, Protocol};
use super::parse::{INET_RESPONSE_HEADER_LEN, NO_INODE, TcpBytes, record, tcp_bytes};
use crate::util::netlink::{DumpSocket, dump_request};

/// What the current tick needs from the socket tables.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SocketNeeds {
    pub traffic: bool,
    pub listening: bool,
    pub connections: bool,
}

impl SocketNeeds {
    pub(super) const fn any(self) -> bool {
        self.traffic || self.listening || self.connections
    }

    pub(super) const fn lists(self) -> bool {
        self.listening || self.connections
    }
}

pub(crate) struct SocketOutputs<'a> {
    pub listening: &'a mut Vec<ListeningPort>,
    pub connections: &'a mut Vec<Connection>,
    pub traffic: &'a mut Vec<ProcessTraffic>,
}

struct Dump {
    protocol: Protocol,
    request: Vec<u8>,
}

#[derive(Debug)]
struct ProcessBytes {
    received: u64,
    acked: u64,
}

pub(crate) struct SocketMonitor {
    channel: DumpSocket,
    dumps: [Dump; 4],
    previous: HashMap<u32, TcpBytes>,
    current: HashMap<u32, TcpBytes>,
    per_process: HashMap<Pid, ProcessBytes>,
    warm: bool,
}

impl SocketMonitor {
    pub(crate) fn open() -> io::Result<Self> {
        let dump = |family, protocol, kind| Dump {
            protocol: kind,
            request: dump_request(SockDiagMessage::InetRequest(InetRequest {
                family,
                protocol,
                extensions: if protocol == IPPROTO_TCP {
                    ExtensionFlags::INFO
                } else {
                    ExtensionFlags::empty()
                },
                states: StateFlags::all(),
                socket_id: if family == AF_INET {
                    SocketId::new_v4()
                } else {
                    SocketId::new_v6()
                },
            })),
        };
        Ok(Self {
            channel: DumpSocket::open(NETLINK_SOCK_DIAG)?,
            dumps: [
                dump(AF_INET, IPPROTO_TCP, Protocol::Tcp),
                dump(AF_INET6, IPPROTO_TCP, Protocol::Tcp),
                dump(AF_INET, IPPROTO_UDP, Protocol::Udp),
                dump(AF_INET6, IPPROTO_UDP, Protocol::Udp),
            ],
            previous: HashMap::new(),
            current: HashMap::new(),
            per_process: HashMap::new(),
            warm: false,
        })
    }

    /// Dumps what `needs` asks for into `outputs` (cleared first). Traffic
    /// rates need `fds` refreshed with the sockets scope; `seconds` is the
    /// time since the previous traffic pass, whose counters are kept even
    /// when ticks in between skip traffic.
    pub(crate) fn sample(
        &mut self,
        needs: SocketNeeds,
        fds: &FdIndex,
        seconds: f64,
        outputs: SocketOutputs<'_>,
    ) -> io::Result<()> {
        outputs.listening.clear();
        outputs.connections.clear();
        outputs.traffic.clear();
        self.current.clear();
        if !needs.any() {
            return Ok(());
        }
        let Self {
            channel,
            dumps,
            current,
            ..
        } = self;
        for dump in dumps.iter_mut() {
            if dump.protocol == Protocol::Udp && !needs.lists() {
                continue;
            }
            let protocol = dump.protocol;
            channel.dump(&mut dump.request, |kind, payload| {
                if kind != SOCK_DIAG_BY_FAMILY {
                    return;
                }
                let Ok(header) = InetResponseHeader::parse(payload) else {
                    return;
                };
                if protocol == Protocol::Tcp
                    && needs.traffic
                    && header.inode != NO_INODE
                    && let Some(bytes) = payload.get(INET_RESPONSE_HEADER_LEN..).and_then(tcp_bytes)
                {
                    current.insert(header.inode, bytes);
                }
                if needs.lists() {
                    record(
                        &header,
                        protocol,
                        needs,
                        fds,
                        &mut *outputs.listening,
                        &mut *outputs.connections,
                    );
                }
            })?;
        }
        if needs.traffic {
            self.attribute_traffic(fds, seconds, outputs.traffic);
        }
        Ok(())
    }

    fn attribute_traffic(
        &mut self,
        fds: &FdIndex,
        seconds: f64,
        traffic: &mut Vec<ProcessTraffic>,
    ) {
        for (inode, bytes) in &self.current {
            let Some(pid) = fds.socket_owner(u64::from(*inode)) else {
                continue;
            };
            let delta = match self.previous.get(inode) {
                Some(previous) => TcpBytes {
                    acked: bytes.acked.saturating_sub(previous.acked),
                    received: bytes.received.saturating_sub(previous.received),
                },
                None if self.warm => *bytes,
                None => TcpBytes::default(),
            };
            let entry = self.per_process.entry(pid).or_insert(ProcessBytes {
                received: 0,
                acked: 0,
            });
            entry.received = entry.received.saturating_add(delta.received);
            entry.acked = entry.acked.saturating_add(delta.acked);
        }
        if seconds > 0.0 {
            traffic.extend(self.per_process.drain().map(|(pid, bytes)| ProcessTraffic {
                pid,
                receive: BytesPerSecond::new(bytes.received as f64 / seconds),
                transmit: BytesPerSecond::new(bytes.acked as f64 / seconds),
                received: Bytes::new(bytes.received),
                transmitted: Bytes::new(bytes.acked),
            }));
        }
        self.per_process.clear();
        std::mem::swap(&mut self.previous, &mut self.current);
        self.warm = true;
    }
}

#[cfg(test)]
mod tests {
    use monitor_core::FdScope;
    use monitor_core::util::procfs::ProcRoot;

    use super::*;
    use crate::net::sockets::model::{BindScope, TcpState};

    fn own_fd_index() -> FdIndex {
        let mut fds = FdIndex::default();
        if let Ok(root) = ProcRoot::open() {
            fds.refresh(
                &root,
                &[Pid::new(std::process::id())],
                FdScope {
                    sockets: true,
                    drm: false,
                },
            );
        }
        fds
    }

    #[test]
    fn sees_own_listener_and_traffic() {
        let Ok(listener) = std::net::TcpListener::bind("127.0.0.1:0") else {
            return;
        };
        let Ok(port) = listener.local_addr().map(|address| address.port()) else {
            return;
        };
        let Ok(mut client) = std::net::TcpStream::connect(("127.0.0.1", port)) else {
            return;
        };
        let Ok((mut server, _)) = listener.accept() else {
            return;
        };
        let Ok(mut monitor) = SocketMonitor::open() else {
            return;
        };
        let fds = own_fd_index();
        let own = Pid::new(std::process::id());
        let needs = SocketNeeds {
            traffic: true,
            listening: true,
            connections: true,
        };
        let (mut listening, mut connections, mut traffic) = (Vec::new(), Vec::new(), Vec::new());
        let sample = |monitor: &mut SocketMonitor,
                      listening: &mut Vec<ListeningPort>,
                      connections: &mut Vec<Connection>,
                      traffic: &mut Vec<ProcessTraffic>| {
            monitor.sample(
                needs,
                &fds,
                1.0,
                SocketOutputs {
                    listening,
                    connections,
                    traffic,
                },
            )
        };
        assert!(sample(&mut monitor, &mut listening, &mut connections, &mut traffic).is_ok());
        let ours = listening
            .iter()
            .find(|entry| entry.protocol == Protocol::Tcp && entry.address.port() == port);
        assert!(ours.is_some_and(|entry| entry.owner == Some(own) && entry.scope == BindScope::Loopback));
        assert!(
            connections
                .iter()
                .any(|connection| connection.remote.port() == port
                    && connection.state == TcpState::Established
                    && connection.owner == Some(own))
        );

        use std::io::{Read, Write};
        let payload = [7_u8; 64 * 1024];
        assert!(client.write_all(&payload).is_ok());
        let mut received = vec![0_u8; payload.len()];
        assert!(server.read_exact(&mut received).is_ok());
        assert!(sample(&mut monitor, &mut listening, &mut connections, &mut traffic).is_ok());
        let own_traffic = traffic.iter().find(|entry| entry.pid == own);
        assert!(
            own_traffic.is_some_and(|entry| entry.receive.get() >= payload.len() as f64
                && entry.transmit.get() >= payload.len() as f64)
        );
    }
}
