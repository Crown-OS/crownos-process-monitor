use std::sync::Arc;
use std::time::Duration;

use monitor_core::util::procfs::ProcRoot;
use monitor_core::util::sysfs::PersistentFile;
use monitor_core::{Bytes, BytesPerSecond, Percent, Pid, ProcessKey, Uid};

use super::files::{IoCounters, SmapsRollup, StatusMemory};
use super::privileged::PrivilegedFigures;
use super::stat::{StatFields, parse_stat};
use crate::apps::ProcessRole;
use crate::apps::cgroup::unit_path;
use crate::apps::resolver::ProcessFacts;
use crate::util::pid_path::PidPath;

mod reads;

/// A PSS reading is kept while RSS stays within 1/16 (about 6 %) of the RSS
/// it was read at… (one smaps_rollup of a large Electron or Node process
/// walks its page tables for ~5 ms)
const ROLLUP_RSS_TOLERANCE: u64 = 16;
/// …for at most this long.
const ROLLUP_REUSE: Duration = Duration::from_secs(300);

/// While minimized, a process that used no CPU since its last read is read
/// on every this many ticks only; its rates then span its own interval.
const IDLE_READ_EVERY: u8 = 6;

/// How often idle processes have their `stat` re-read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IdleReads {
    EveryTick,
    /// Nobody sees the table; history and alerts only need CPU seconds.
    Spaced,
}

/// Facts shared by every process update in one tick.
pub(crate) struct TickFacts {
    pub generation: u64,
    /// Seconds since the previous sample; `None` when rates are invalid.
    pub seconds: Option<f64>,
    /// Monotonic time of this tick.
    pub now: Duration,
    pub idle_reads: IdleReads,
    pub ticks_per_second: u64,
    pub page_size: u64,
}

pub(crate) enum Refresh {
    Alive,
    /// `comm` changed: the process called `execve`.
    Executed,
    Gone,
}

pub(crate) enum IoState {
    Untried,
    Denied,
    Open {
        file: PersistentFile,
        previous: Option<IoCounters>,
        read: BytesPerSecond,
        write: BytesPerSecond,
    },
}

/// Everything the collector keeps about one live process between ticks.
pub(crate) struct ProcessEntry {
    pub key: ProcessKey,
    stat_file: PersistentFile,
    pub seen: u64,
    pub stat: StatFields,
    pub comm: Arc<str>,
    pub command_line: Arc<str>,
    pub executable: Option<Arc<str>>,
    pub cgroup: Arc<str>,
    pub uid: Uid,
    pub role: ProcessRole,
    pub app: Option<usize>,
    /// The app's own cgroup (the unit path) when this process lives in it.
    pub scope_path: Option<Arc<str>>,
    pub started_unix: u64,
    previous_cpu_ticks: Option<u64>,
    previous_cpu_at: Duration,
    /// Ticks skipped since the last `stat` read of an idle process.
    skipped_reads: u8,
    pub cpu: Percent,
    pub resident: Bytes,
    pub peak_seen: Bytes,
    pub rollup: Option<SmapsRollup>,
    /// Round-robin pass in which `rollup` was read or found still valid.
    pub rollup_cycle: u64,
    /// Monotonic time `rollup` was actually read.
    rollup_read_at: Duration,
    /// RSS when `rollup` was read.
    rollup_resident: Bytes,
    pub rollup_denied: bool,
    pub status: Option<StatusMemory>,
    pub io: IoState,
    pub privileged: Option<PrivilegedFigures>,
}

impl ProcessEntry {
    pub fn open(
        root: &ProcRoot,
        pid: Pid,
        buffer: &mut Vec<u8>,
        tick: &TickFacts,
        now_unix: u64,
    ) -> Option<Self> {
        let stat_file =
            PersistentFile::open_at(root.as_fd(), PidPath::new(pid, "stat").as_str()).ok()?;
        let (comm, stat) = {
            let (comm, stat) = parse_stat(stat_file.read(buffer).ok()?)?;
            (Arc::from(&*String::from_utf8_lossy(comm)), stat)
        };
        let started_after_boot = stat.start_ticks / tick.ticks_per_second.max(1);
        let age = crate::util::clock::since_boot()
            .as_secs()
            .saturating_sub(started_after_boot);
        let mut entry = Self {
            key: ProcessKey {
                pid,
                start_ticks: stat.start_ticks,
            },
            stat_file,
            seen: tick.generation,
            stat,
            comm,
            command_line: Arc::from(""),
            executable: None,
            cgroup: Arc::from("/"),
            uid: Uid::ROOT,
            role: ProcessRole::Main,
            app: None,
            scope_path: None,
            started_unix: now_unix.saturating_sub(age),
            previous_cpu_ticks: None,
            previous_cpu_at: tick.now,
            skipped_reads: 0,
            cpu: Percent::ZERO,
            resident: Bytes::ZERO,
            peak_seen: Bytes::ZERO,
            rollup: None,
            rollup_cycle: 0,
            rollup_read_at: Duration::ZERO,
            rollup_resident: Bytes::ZERO,
            rollup_denied: false,
            status: None,
            io: IoState::Untried,
            privileged: None,
        };
        entry.read_identity(root, buffer);
        entry.update_figures(stat, tick);
        Some(entry)
    }

    pub fn refresh(&mut self, buffer: &mut Vec<u8>, tick: &TickFacts) -> Refresh {
        if self.may_skip_read(tick) {
            self.skipped_reads += 1;
            self.seen = tick.generation;
            return Refresh::Alive;
        }
        self.skipped_reads = 0;
        let Some((comm, stat)) = self.stat_file.read(buffer).ok().and_then(parse_stat) else {
            return Refresh::Gone;
        };
        if stat.start_ticks != self.key.start_ticks {
            return Refresh::Gone;
        }
        let executed = comm != self.comm.as_bytes();
        if executed {
            self.comm = Arc::from(&*String::from_utf8_lossy(comm));
        }
        self.update_figures(stat, tick);
        self.expire_privileged(tick.generation);
        if executed {
            Refresh::Executed
        } else {
            Refresh::Alive
        }
    }

    fn may_skip_read(&self, tick: &TickFacts) -> bool {
        tick.idle_reads == IdleReads::Spaced
            && tick.seconds.is_some()
            && self.cpu == Percent::ZERO
            && self.previous_cpu_ticks.is_some()
            && self.skipped_reads + 1 < IDLE_READ_EVERY
    }

    fn update_figures(&mut self, stat: StatFields, tick: &TickFacts) {
        self.seen = tick.generation;
        let ticks = stat.cpu_ticks();
        let own_seconds = tick.now.saturating_sub(self.previous_cpu_at).as_secs_f64();
        self.cpu = match (self.previous_cpu_ticks, tick.seconds) {
            (Some(previous), Some(_)) if own_seconds > 0.0 => Percent::from_ratio(
                ticks.saturating_sub(previous) as f64 / tick.ticks_per_second as f64 / own_seconds,
            ),
            _ => Percent::ZERO,
        };
        self.previous_cpu_ticks = Some(ticks);
        self.previous_cpu_at = tick.now;
        self.resident = Bytes::new(stat.resident_pages.saturating_mul(tick.page_size));
        self.peak_seen = self.peak_seen.max(self.resident);
        self.stat = stat;
    }

    /// Records that the process sits in its app's own unit.
    pub fn claim_scope(&mut self) {
        let (unit, _) = unit_path(&self.cgroup);
        self.scope_path = Some(if unit.len() == self.cgroup.len() {
            self.cgroup.clone()
        } else {
            Arc::from(unit)
        });
    }

    pub fn facts(&self) -> ProcessFacts<'_> {
        ProcessFacts {
            uid: self.uid,
            name: &self.comm,
            executable: self.executable.as_deref(),
            command_line: &self.command_line,
            cgroup: &self.cgroup,
            kernel_thread: self.stat.is_kernel_thread(),
        }
    }

    /// Where ancestry stops: PID 1 and `systemd --user`, or the leader of a
    /// login session.
    pub fn is_ancestry_boundary(&self) -> bool {
        &*self.comm == "systemd"
            || (self.stat.session == self.key.pid.get()
                && crate::apps::cgroup::leaf(&self.cgroup).starts_with("session-"))
    }

    pub fn peak_memory(&self) -> Bytes {
        self.peak_seen
            .max(self.status.map_or(Bytes::ZERO, |status| status.peak))
    }
}
