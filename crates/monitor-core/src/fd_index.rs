//! One shared walk over `/proc/<pid>/fd` that serves every collector needing
//! to know which process holds a socket or a DRM device.

use std::collections::HashMap;
use std::mem::MaybeUninit;

use rustix::fs::{AtFlags, Mode, OFlags, RawDir};

use crate::Pid;
use crate::util::parse;
use crate::util::procfs::{PidName, ProcRoot};

const DIRENT_BUFFER: usize = 16 * 1024;
/// [`FdIndex::refresh_changed`] rescans every process on this refresh and
/// every this many after, catching descriptors swapped at a constant count
/// and DRM devices opened late.
const FULL_RESCAN_EVERY: u64 = 30;
const FD_PATH_CAPACITY: usize = 16;
const LINK_BUFFER: usize = 128;
const SOCKET_PREFIX: &[u8] = b"socket:[";
const DRM_PREFIX: &[u8] = b"/dev/dri/";

/// Which processes a rebuild walks the descriptors of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Walk {
    Every,
    NewOrChanged,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FdScope {
    pub sockets: bool,
    pub drm: bool,
}

impl FdScope {
    #[must_use]
    pub const fn is_empty(self) -> bool {
        !self.sockets && !self.drm
    }
}

/// What one process's descriptors held when they were last walked; a walk
/// records both scopes, whichever was asked for.
#[derive(Debug, Default)]
struct ProcessFds {
    fd_count: u64,
    sockets: Vec<u64>,
    drm: Vec<u32>,
    refresh: u64,
}

/// An open DRM device file descriptor, whose `/proc/<pid>/fdinfo/<fd>`
/// carries the client's GPU usage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrmFd {
    pub pid: Pid,
    pub fd: u32,
}

pub struct FdIndex {
    sockets: HashMap<u64, Pid>,
    drm: Vec<DrmFd>,
    scope: FdScope,
    processes: HashMap<Pid, ProcessFds>,
    refreshes: u64,
    dirent_buffer: Box<[MaybeUninit<u8>]>,
}

impl Default for FdIndex {
    fn default() -> Self {
        Self {
            sockets: HashMap::new(),
            drm: Vec::new(),
            scope: FdScope::default(),
            processes: HashMap::new(),
            refreshes: 0,
            dirent_buffer: Box::new_uninit_slice(DIRENT_BUFFER),
        }
    }
}

impl std::fmt::Debug for FdIndex {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FdIndex")
            .field("sockets", &self.sockets.len())
            .field("drm", &self.drm.len())
            .field("scope", &self.scope)
            .finish()
    }
}

impl FdIndex {
    #[must_use]
    pub fn socket_owner(&self, inode: u64) -> Option<Pid> {
        self.sockets.get(&inode).copied()
    }

    /// Every `(socket inode, owner)` pair; a shared socket has one owner.
    #[must_use]
    pub fn sockets(&self) -> impl ExactSizeIterator<Item = (u64, Pid)> + '_ {
        self.sockets.iter().map(|(&inode, &pid)| (inode, pid))
    }

    #[must_use]
    pub fn drm_fds(&self) -> &[DrmFd] {
        &self.drm
    }

    /// Replaces the sockets scope with owners found elsewhere (the
    /// privileged helper sees every user's descriptors).
    pub fn replace_sockets(&mut self, owners: impl IntoIterator<Item = (u64, Pid)>) {
        self.sockets.clear();
        self.sockets.extend(owners);
        self.scope.sockets = true;
    }

    /// What the last refresh collected.
    #[must_use]
    pub fn scope(&self) -> FdScope {
        self.scope
    }

    /// Rebuilds the index for `pids` by walking every descriptor. Processes
    /// whose descriptors cannot be read (other users without the helper)
    /// are skipped silently.
    pub fn refresh(&mut self, root: &ProcRoot, pids: &[Pid], scope: FdScope) {
        self.rebuild(root, pids, scope, Walk::Every);
    }

    /// Like [`FdIndex::refresh`], but walks only processes not seen before
    /// and, for the sockets scope, those whose descriptor count changed (one
    /// `stat` each), with a full walk every few refreshes. Processes open
    /// their DRM devices at start, so the DRM scope alone costs no syscall
    /// for known processes.
    pub fn refresh_changed(&mut self, root: &ProcRoot, pids: &[Pid], scope: FdScope) {
        let walk = if self.refreshes.is_multiple_of(FULL_RESCAN_EVERY) {
            Walk::Every
        } else {
            Walk::NewOrChanged
        };
        self.rebuild(root, pids, scope, walk);
    }

    fn rebuild(&mut self, root: &ProcRoot, pids: &[Pid], scope: FdScope, walk: Walk) {
        self.sockets.clear();
        self.drm.clear();
        self.scope = scope;
        if scope.is_empty() {
            return;
        }
        self.refreshes += 1;
        let refresh = self.refreshes;
        for &pid in pids {
            let fd_count = if scope.sockets {
                let Some(count) = fd_count(root, pid) else {
                    continue;
                };
                count
            } else {
                0
            };
            let cached = self.processes.entry(pid).or_default();
            let stale = walk == Walk::Every
                || cached.refresh == 0
                || (scope.sockets && cached.fd_count != fd_count);
            if stale {
                scan_process(root, pid, cached, &mut self.dirent_buffer);
            }
            cached.refresh = refresh;
            if scope.sockets {
                self.sockets
                    .extend(cached.sockets.iter().map(|&inode| (inode, pid)));
            }
            if scope.drm {
                self.drm
                    .extend(cached.drm.iter().map(|&fd| DrmFd { pid, fd }));
            }
        }
        self.processes.retain(|_, cached| cached.refresh == refresh);
    }
}

/// `st_size` of `/proc/<pid>/fd` is the number of open descriptors.
fn fd_count(root: &ProcRoot, pid: Pid) -> Option<u64> {
    let mut path = [0_u8; FD_PATH_CAPACITY];
    let mut name = PidName::new(pid);
    let name = name.as_str().as_bytes();
    let length = name.len() + b"/fd".len();
    path.get_mut(..name.len())?.copy_from_slice(name);
    path.get_mut(name.len()..length)?.copy_from_slice(b"/fd");
    let path = std::str::from_utf8(&path[..length]).ok()?;
    rustix::fs::statat(root.as_fd(), path, AtFlags::empty())
        .ok()
        .and_then(|stat| u64::try_from(stat.st_size).ok())
}

fn scan_process(
    root: &ProcRoot,
    pid: Pid,
    cached: &mut ProcessFds,
    dirent_buffer: &mut [MaybeUninit<u8>],
) {
    cached.sockets.clear();
    cached.drm.clear();
    let Ok(process) = root.open_pid(pid) else {
        return;
    };
    let Ok(directory) = rustix::fs::openat(
        &process,
        "fd",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    ) else {
        return;
    };
    cached.fd_count = rustix::fs::fstat(&directory)
        .ok()
        .and_then(|stat| u64::try_from(stat.st_size).ok())
        .unwrap_or(0);
    let mut link = [0_u8; LINK_BUFFER];
    let mut entries = RawDir::new(&directory, dirent_buffer);
    while let Some(Ok(entry)) = entries.next() {
        let name = entry.file_name();
        let Some(fd) = parse::parse_u32(name.to_bytes()) else {
            continue;
        };
        let Ok(length) = rustix::fs::readlinkat_raw(&directory, name, &mut link[..]) else {
            continue;
        };
        let target = &link[..length.min(LINK_BUFFER)];
        if let Some(inode) = socket_inode(target) {
            cached.sockets.push(inode);
        } else if target.starts_with(DRM_PREFIX) {
            cached.drm.push(fd);
        }
    }
}

fn socket_inode(target: &[u8]) -> Option<u64> {
    target
        .strip_prefix(SOCKET_PREFIX)?
        .strip_suffix(b"]")
        .and_then(parse::parse_u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_socket_links() {
        assert_eq!(socket_inode(b"socket:[12345]"), Some(12345));
        assert_eq!(socket_inode(b"pipe:[12345]"), None);
        assert_eq!(socket_inode(b"socket:[12"), None);
    }

    #[test]
    fn finds_own_socket() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0");
        assert!(listener.is_ok());
        let Ok(root) = ProcRoot::open() else { return };
        let own = Pid::new(std::process::id());
        let mut index = FdIndex::default();
        index.refresh(
            &root,
            &[own],
            FdScope {
                sockets: true,
                drm: false,
            },
        );
        assert!(index.sockets().any(|(_, pid)| pid == own));
        let sockets = index.sockets().len();
        index.refresh_changed(
            &root,
            &[own],
            FdScope {
                sockets: true,
                drm: false,
            },
        );
        assert_eq!(
            index.sockets().len(),
            sockets,
            "an unchanged process is reused"
        );
        index.replace_sockets([(7, own)]);
        assert_eq!(index.socket_owner(7), Some(own));
        assert!(index.scope().sockets);
    }
}
