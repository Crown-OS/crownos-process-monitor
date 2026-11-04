use std::fmt::Write;

use monitor_core::util::procfs::ProcRoot;
use monitor_core::util::sysfs;
use monitor_core::{FdIndex, FdScope, Pid};

use crate::{DrmClient, HelperError, SocketOwner};

const SOCKETS_ONLY: FdScope = FdScope {
    sockets: true,
    drm: false,
};
const DRM_ONLY: FdScope = FdScope {
    sockets: false,
    drm: true,
};

/// Walks every process's descriptors with root's view.
pub(crate) struct DescriptorReader {
    root: ProcRoot,
    pids: Vec<Pid>,
    /// One index per scope, so each refreshes incrementally.
    sockets: FdIndex,
    drm: FdIndex,
    buffer: Vec<u8>,
    fdinfo_name: String,
}

impl DescriptorReader {
    pub(crate) fn open() -> Result<Self, HelperError> {
        Ok(Self {
            root: ProcRoot::open().map_err(HelperError::failed)?,
            pids: Vec::new(),
            sockets: FdIndex::default(),
            drm: FdIndex::default(),
            buffer: Vec::new(),
            fdinfo_name: String::new(),
        })
    }

    fn list_pids(&mut self) -> Result<(), HelperError> {
        self.root
            .list_pids(&mut self.pids)
            .map_err(HelperError::failed)
    }

    /// The process holding each socket inode; a socket shared by several
    /// processes is reported once.
    pub(crate) fn socket_owners(&mut self) -> Result<Vec<SocketOwner>, HelperError> {
        self.list_pids()?;
        self.sockets
            .refresh_changed(&self.root, &self.pids, SOCKETS_ONLY);
        Ok(self
            .sockets
            .sockets()
            .map(|(inode, pid)| SocketOwner {
                inode,
                pid: pid.get(),
            })
            .collect())
    }

    /// The fdinfo text of every open DRM device of every process.
    pub(crate) fn drm_clients(&mut self) -> Result<Vec<DrmClient>, HelperError> {
        self.list_pids()?;
        self.drm.refresh_changed(&self.root, &self.pids, DRM_ONLY);
        let mut clients = Vec::with_capacity(self.drm.drm_fds().len());
        for drm in self.drm.drm_fds() {
            let Ok(process) = self.root.open_pid(drm.pid) else {
                continue;
            };
            self.fdinfo_name.clear();
            if write!(self.fdinfo_name, "fdinfo/{}", drm.fd).is_err() {
                continue;
            }
            if let Ok(text) = sysfs::read_at(&process, &self.fdinfo_name, &mut self.buffer) {
                clients.push(DrmClient {
                    pid: drm.pid.get(),
                    fd: drm.fd,
                    fdinfo: String::from_utf8_lossy(text).into_owned(),
                });
            }
        }
        Ok(clients)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_own_sockets() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0");
        assert!(listener.is_ok());
        let Ok(mut reader) = DescriptorReader::open() else {
            return;
        };
        let owners = reader.socket_owners().unwrap_or_default();
        assert!(owners.iter().any(|owner| owner.pid == std::process::id()));
    }

    #[test]
    fn reads_drm_fdinfo_when_present() {
        let Ok(mut reader) = DescriptorReader::open() else {
            return;
        };
        let clients = reader.drm_clients().unwrap_or_default();
        assert!(clients.iter().all(|client| client.fdinfo.contains("pos:")));
    }
}
