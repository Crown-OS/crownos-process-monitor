//! Mount table changes without polling the file: the kernel raises
//! `POLLPRI` on an open `/proc/self/mountinfo` whenever a mount appears or
//! goes. A zero-timeout `poll` per tick checks the flag.

use std::os::fd::OwnedFd;

use monitor_core::util::sysfs::PersistentFile;
use monitor_core::{SourceError, SourceResult};
use rustix::event::{PollFd, PollFlags, Timespec};
use rustix::fs::{Mode, OFlags};

const MOUNTINFO: &str = "/proc/self/mountinfo";
const NO_WAIT: Timespec = Timespec {
    tv_sec: 0,
    tv_nsec: 0,
};

#[derive(Debug)]
pub(crate) struct MountWatch {
    watched: OwnedFd,
    content: PersistentFile,
}

impl MountWatch {
    pub(crate) fn open() -> SourceResult<Self> {
        let watched = rustix::fs::open(MOUNTINFO, OFlags::RDONLY | OFlags::CLOEXEC, Mode::empty())
            .map_err(|errno| SourceError::io(MOUNTINFO, errno))?;
        Ok(Self {
            watched,
            content: PersistentFile::open(MOUNTINFO)?,
        })
    }

    /// True once after every change of the mount table.
    pub(crate) fn changed(&self) -> bool {
        let mut descriptors = [PollFd::new(&self.watched, PollFlags::PRI)];
        if rustix::event::poll(&mut descriptors, Some(&NO_WAIT)).is_err() {
            return false;
        }
        descriptors[0]
            .revents()
            .intersects(PollFlags::PRI | PollFlags::ERR)
    }

    pub(crate) fn read<'b>(&self, buffer: &'b mut Vec<u8>) -> SourceResult<&'b [u8]> {
        self.content.read(buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_table_reports_no_change() {
        let Ok(watch) = MountWatch::open() else {
            return;
        };
        let mut buffer = Vec::new();
        assert!(watch.read(&mut buffer).is_ok_and(|text| !text.is_empty()));
        assert!(!watch.changed());
    }
}
