//! Process enumeration through an open `/proc` directory with a reusable
//! `getdents64` buffer, so listing ~1000 processes allocates nothing.

use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::sync::Arc;

use rustix::fs::{Mode, OFlags, RawDir, SeekFrom};

use super::parse;
use super::sysfs::read_at;
use crate::{Pid, SourceError, SourceResult};

const DIRENT_BUFFER: usize = 32 * 1024;
const PROC: &str = "/proc";
const BOOT_ID: &str = "sys/kernel/random/boot_id";
const BOOT_ID_LENGTH: usize = 36;

#[derive(Debug)]
pub struct ProcRoot {
    fd: OwnedFd,
    dirent_buffer: Box<[MaybeUninit<u8>]>,
}

impl ProcRoot {
    pub fn open() -> SourceResult<Self> {
        let fd = rustix::fs::open(
            PROC,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|errno| SourceError::io(PROC, errno))?;
        Ok(Self {
            fd,
            dirent_buffer: Box::new_uninit_slice(DIRENT_BUFFER),
        })
    }

    #[must_use]
    pub fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }

    /// Replaces `out` with every PID currently in `/proc`, ascending.
    pub fn list_pids(&mut self, out: &mut Vec<Pid>) -> SourceResult<()> {
        out.clear();
        rustix::fs::seek(&self.fd, SeekFrom::Start(0))
            .map_err(|errno| SourceError::io(PROC, errno))?;
        let mut entries = RawDir::new(&self.fd, &mut self.dirent_buffer);
        while let Some(entry) = entries.next() {
            let entry = entry.map_err(|errno| SourceError::io(PROC, errno))?;
            if let Some(pid) = parse::parse_u32(entry.file_name().to_bytes()) {
                out.push(Pid::new(pid));
            }
        }
        out.sort_unstable();
        Ok(())
    }

    /// This boot's random id, e.g. `5f0c…-…`, which changes on every reboot.
    pub fn boot_id(&self) -> SourceResult<Arc<str>> {
        let mut buffer = Vec::with_capacity(BOOT_ID_LENGTH + 1);
        let text = read_at(&self.fd, BOOT_ID, &mut buffer)?;
        parse::as_str(text.trim_ascii())
            .filter(|id| !id.is_empty())
            .map(Arc::from)
            .ok_or(SourceError::parse("boot_id"))
    }

    /// Opens `/proc/<pid>` as a directory, for `openat` of its files.
    pub fn open_pid(&self, pid: Pid) -> SourceResult<OwnedFd> {
        let mut name = PidName::new(pid);
        rustix::fs::openat(
            &self.fd,
            name.as_str(),
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|errno| SourceError::io("/proc/<pid>", errno))
    }
}

/// A PID formatted into a stack buffer, usable as a relative path.
pub struct PidName {
    bytes: [u8; 10],
    start: usize,
}

impl PidName {
    #[must_use]
    pub fn new(pid: Pid) -> Self {
        Self::from_number(pid.get())
    }

    #[must_use]
    pub fn from_number(mut number: u32) -> Self {
        let mut bytes = [0_u8; 10];
        let mut start = bytes.len();
        loop {
            start -= 1;
            bytes[start] = b'0' + (number % 10) as u8;
            number /= 10;
            if number == 0 {
                break;
            }
        }
        Self { bytes, start }
    }

    pub fn as_str(&mut self) -> &str {
        std::str::from_utf8(&self.bytes[self.start..]).unwrap_or("0")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_own_process() {
        let own = Pid::new(std::process::id());
        let mut root = match ProcRoot::open() {
            Ok(root) => root,
            Err(error) => panic!("cannot open /proc: {error}"),
        };
        let mut pids = Vec::new();
        assert!(root.list_pids(&mut pids).is_ok());
        assert!(pids.binary_search(&own).is_ok());
        assert!(root.list_pids(&mut pids).is_ok());
        assert!(pids.binary_search(&own).is_ok(), "second listing rewinds");
        assert!(root.open_pid(own).is_ok());
        assert!(root.boot_id().is_ok_and(|id| id.len() == BOOT_ID_LENGTH));
    }

    #[test]
    fn formats_pid_names() {
        assert_eq!(PidName::from_number(0).as_str(), "0");
        assert_eq!(PidName::from_number(4_294_967_295).as_str(), "4294967295");
    }
}
