//! Thin request/response and event plumbing over `netlink-sys` sockets with
//! reusable buffers: a dump allocates nothing once the buffers are warm.

use std::io;

use netlink_packet_core::{
    NLM_F_DUMP, NLM_F_REQUEST, NLMSG_DONE, NLMSG_ERROR, NetlinkBuffer, NetlinkHeader,
    NetlinkMessage, NetlinkPayload, NetlinkSerializable,
};
use netlink_sys::{Socket, SocketAddr};

/// Kernel dump skbs are at most 32 KiB; twice that never truncates.
const RECEIVE_BUFFER: usize = 64 * 1024;
const EVENT_BUFFER: usize = 8 * 1024;
const SEQUENCE_BYTES: std::ops::Range<usize> = 8..12;
const ALIGNMENT: usize = 4;
const ERROR_CODE_BYTES: usize = 4;

/// Serializes a dump request once; [`DumpSocket::dump`] patches the
/// sequence number in place for every send.
pub(crate) fn dump_request<Inner>(inner: Inner) -> Vec<u8>
where
    Inner: NetlinkSerializable,
{
    let mut header = NetlinkHeader::default();
    header.flags = NLM_F_REQUEST | NLM_F_DUMP;
    let mut message = NetlinkMessage::new(header, NetlinkPayload::InnerMessage(inner));
    message.finalize();
    let mut bytes = vec![0; message.buffer_len()];
    message.serialize(&mut bytes);
    bytes
}

#[derive(Debug)]
pub(crate) struct DumpSocket {
    socket: Socket,
    sequence: u32,
    receive: Vec<u8>,
}

impl DumpSocket {
    pub(crate) fn open(protocol: isize) -> io::Result<Self> {
        let mut socket = Socket::new(protocol)?;
        socket.bind_auto()?;
        socket.connect(&SocketAddr::new(0, 0))?;
        Ok(Self {
            socket,
            sequence: 0,
            receive: Vec::with_capacity(RECEIVE_BUFFER),
        })
    }

    /// Sends `request` and hands every reply message (type and payload) to
    /// `on_message` until the kernel signals the end of the dump.
    pub(crate) fn dump(
        &mut self,
        request: &mut [u8],
        mut on_message: impl FnMut(u16, &[u8]),
    ) -> io::Result<()> {
        self.sequence = self.sequence.wrapping_add(1);
        let Some(sequence_field) = request.get_mut(SEQUENCE_BYTES) else {
            return Err(io::ErrorKind::InvalidInput.into());
        };
        sequence_field.copy_from_slice(&self.sequence.to_ne_bytes());
        self.socket.send(request, 0)?;
        loop {
            self.receive.clear();
            self.socket.recv(&mut self.receive, 0)?;
            if let Some(outcome) = dispatch(&self.receive, self.sequence, &mut on_message) {
                return outcome;
            }
        }
    }
}

/// Walks the messages of one datagram; `Some` once the dump finished.
fn dispatch(
    datagram: &[u8],
    sequence: u32,
    on_message: &mut impl FnMut(u16, &[u8]),
) -> Option<io::Result<()>> {
    let mut offset = 0;
    while let Some(rest) = datagram.get(offset..) {
        let Ok(message) = NetlinkBuffer::new_checked(rest) else {
            break;
        };
        let length = message.length() as usize;
        if message.sequence_number() == sequence {
            match message.message_type() {
                NLMSG_DONE => return Some(Ok(())),
                NLMSG_ERROR => return Some(Err(error_reply(message.payload()))),
                kind => on_message(kind, message.payload()),
            }
        }
        if length == 0 {
            break;
        }
        offset += length.next_multiple_of(ALIGNMENT);
    }
    None
}

fn error_reply(payload: &[u8]) -> io::Error {
    let code = payload
        .get(..ERROR_CODE_BYTES)
        .and_then(|bytes| bytes.try_into().ok())
        .map_or(0, i32::from_ne_bytes);
    if code == 0 {
        io::ErrorKind::InvalidData.into()
    } else {
        io::Error::from_raw_os_error(code.saturating_neg())
    }
}

/// A non-blocking socket subscribed to multicast groups, drained once per
/// tick to learn whether anything changed.
#[derive(Debug)]
pub(crate) struct EventSocket {
    socket: Socket,
    receive: Vec<u8>,
}

impl EventSocket {
    pub(crate) fn open(protocol: isize, groups: u32) -> io::Result<Self> {
        let mut socket = Socket::new(protocol)?;
        socket.bind(&SocketAddr::new(0, groups))?;
        socket.set_non_blocking(true)?;
        Ok(Self {
            socket,
            receive: Vec::with_capacity(EVENT_BUFFER),
        })
    }

    /// Discards queued events and reports whether there were any. An
    /// overflowed queue (`ENOBUFS`) counts as a change.
    pub(crate) fn drain(&mut self) -> bool {
        let mut changed = false;
        loop {
            self.receive.clear();
            match self.socket.recv(&mut self.receive, 0) {
                Ok(_) => changed = true,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return changed,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => return true,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(kind: u16, sequence: u32, payload: &[u8]) -> Vec<u8> {
        let length = 16 + payload.len();
        let mut bytes = Vec::with_capacity(length.next_multiple_of(ALIGNMENT));
        bytes.extend_from_slice(&(length as u32).to_ne_bytes());
        bytes.extend_from_slice(&kind.to_ne_bytes());
        bytes.extend_from_slice(&0_u16.to_ne_bytes());
        bytes.extend_from_slice(&sequence.to_ne_bytes());
        bytes.extend_from_slice(&0_u32.to_ne_bytes());
        bytes.extend_from_slice(payload);
        bytes.resize(length.next_multiple_of(ALIGNMENT), 0);
        bytes
    }

    #[test]
    fn dispatches_until_done() {
        let mut datagram = message(16, 7, b"abc");
        datagram.extend(message(16, 6, b"stale"));
        datagram.extend(message(NLMSG_DONE, 7, &[0; 4]));
        let mut seen = Vec::new();
        let outcome = dispatch(&datagram, 7, &mut |kind, payload| {
            seen.push((kind, payload.to_vec()))
        });
        assert!(matches!(outcome, Some(Ok(()))));
        assert_eq!(seen, [(16, b"abc".to_vec())]);
    }

    #[test]
    fn reports_kernel_errors() {
        let datagram = message(NLMSG_ERROR, 1, &(-13_i32).to_ne_bytes());
        let outcome = dispatch(&datagram, 1, &mut |_, _| {});
        assert!(
            matches!(outcome, Some(Err(error)) if error.kind() == io::ErrorKind::PermissionDenied)
        );
    }

    #[test]
    fn waits_for_more_datagrams() {
        let datagram = message(16, 2, b"part");
        assert!(dispatch(&datagram, 2, &mut |_, _| {}).is_none());
    }
}
