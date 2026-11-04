use std::sync::Arc;
use std::time::Duration;

use monitor_core::BytesPerSecond;
use monitor_core::Uid;
use monitor_core::util::procfs::{PidName, ProcRoot};
use monitor_core::util::sysfs::{PersistentFile, read_at};
use rustix::fs::AtFlags;

use super::{IoState, ProcessEntry, ROLLUP_REUSE, ROLLUP_RSS_TOLERANCE};
use crate::apps::ProcessRole;
use crate::apps::cgroup::unified_path;
use crate::process::files::{IoCounters, SmapsRollup, StatusMemory};
use crate::util::pid_path::PidPath;
use crate::util::text::display_command_line;

const LINK_BUFFER: usize = 4096;

impl ProcessEntry {
    /// Reads what only changes on `execve`: uid, command line, executable
    /// and cgroup.
    pub fn read_identity(&mut self, root: &ProcRoot, buffer: &mut Vec<u8>) {
        let pid = self.key.pid;
        if let Ok(stat) =
            rustix::fs::statat(root.as_fd(), PidName::new(pid).as_str(), AtFlags::empty())
        {
            self.uid = Uid::new(stat.st_uid);
        }
        self.rollup = None;
        self.rollup_denied = false;
        self.status = None;
        self.io = IoState::Untried;
        self.privileged = None;
        self.app = None;
        self.scope_path = None;
        if !self.stat.is_kernel_thread() {
            if let Ok(raw) = read_at(root.as_fd(), PidPath::new(pid, "cmdline").as_str(), buffer) {
                self.role = ProcessRole::from_command_line(raw);
                self.command_line = display_command_line(raw);
            }
            let mut link = [0_u8; LINK_BUFFER];
            self.executable = rustix::fs::readlinkat_raw(
                root.as_fd(),
                PidPath::new(pid, "exe").as_str(),
                &mut link[..],
            )
            .ok()
            .map(|length| Arc::from(&*String::from_utf8_lossy(&link[..length.min(LINK_BUFFER)])));
        }
        self.reread_cgroup(root, buffer);
    }

    /// Launchers often move a fresh process into its app scope right after
    /// `fork`, so young processes have their cgroup re-read. Returns whether
    /// it changed.
    pub fn reread_cgroup(&mut self, root: &ProcRoot, buffer: &mut Vec<u8>) -> bool {
        let Ok(raw) = read_at(
            root.as_fd(),
            PidPath::new(self.key.pid, "cgroup").as_str(),
            buffer,
        ) else {
            return false;
        };
        let path = unified_path(std::str::from_utf8(raw).unwrap_or("0::/"));
        if path == &*self.cgroup {
            return false;
        }
        self.cgroup = Arc::from(path);
        true
    }

    pub fn read_rollup(
        &mut self,
        root: &ProcRoot,
        buffer: &mut Vec<u8>,
        cycle: u64,
        now: Duration,
    ) {
        match read_at(
            root.as_fd(),
            PidPath::new(self.key.pid, "smaps_rollup").as_str(),
            buffer,
        ) {
            Ok(bytes) => {
                self.rollup = Some(SmapsRollup::parse(bytes));
                self.rollup_cycle = cycle;
                self.rollup_read_at = now;
                self.rollup_resident = self.resident;
            }
            Err(error) => self.rollup_denied = error.is_permission_denied(),
        }
    }

    /// PSS barely moves while RSS holds still, so a rollup read at a close
    /// RSS stays valid for a while; returns whether it was kept.
    pub fn revalidate_rollup(&mut self, cycle: u64, now: Duration) -> bool {
        let drift = self.resident.get().abs_diff(self.rollup_resident.get());
        let kept = self.rollup.is_some()
            && drift <= self.rollup_resident.get() / ROLLUP_RSS_TOLERANCE
            && now.saturating_sub(self.rollup_read_at) < ROLLUP_REUSE;
        if kept {
            self.rollup_cycle = cycle;
        }
        kept
    }

    pub fn read_status(&mut self, root: &ProcRoot, buffer: &mut Vec<u8>) {
        if let Ok(bytes) = read_at(
            root.as_fd(),
            PidPath::new(self.key.pid, "status").as_str(),
            buffer,
        ) {
            self.status = Some(StatusMemory::parse(bytes));
        }
    }

    pub fn read_io(&mut self, root: &ProcRoot, buffer: &mut Vec<u8>, seconds: Option<f64>) {
        if matches!(self.io, IoState::Untried) {
            self.io = match PersistentFile::open_at(
                root.as_fd(),
                PidPath::new(self.key.pid, "io").as_str(),
            ) {
                Ok(file) => IoState::Open {
                    file,
                    previous: None,
                    read: BytesPerSecond::ZERO,
                    write: BytesPerSecond::ZERO,
                },
                Err(_) => IoState::Denied,
            };
        }
        let IoState::Open {
            file,
            previous,
            read,
            write,
        } = &mut self.io
        else {
            return;
        };
        let Ok(bytes) = file.read(buffer) else {
            self.io = IoState::Denied;
            return;
        };
        let counters = IoCounters::parse(bytes);
        (*read, *write) = match (previous.replace(counters), seconds) {
            (Some(earlier), Some(seconds)) => (
                BytesPerSecond::between(earlier.read_bytes, counters.read_bytes, seconds),
                BytesPerSecond::between(earlier.write_bytes, counters.write_bytes, seconds),
            ),
            _ => (BytesPerSecond::ZERO, BytesPerSecond::ZERO),
        };
    }

    /// Forgets the I/O baseline while nobody looks at I/O.
    pub fn pause_io(&mut self) {
        if let Some(figures) = self.privileged.as_mut() {
            figures.read = BytesPerSecond::ZERO;
            figures.write = BytesPerSecond::ZERO;
        }
        if let IoState::Open {
            previous,
            read,
            write,
            ..
        } = &mut self.io
        {
            *previous = None;
            *read = BytesPerSecond::ZERO;
            *write = BytesPerSecond::ZERO;
        }
    }

    pub fn io_rates(&self) -> (BytesPerSecond, BytesPerSecond) {
        if let Some(rates) = self.privileged_io() {
            return rates;
        }
        match &self.io {
            IoState::Open { read, write, .. } => (*read, *write),
            IoState::Untried | IoState::Denied => (BytesPerSecond::ZERO, BytesPerSecond::ZERO),
        }
    }
}
