use std::sync::Arc;
use std::time::Duration;

use monitor_core::{AppKey, Bytes, BytesPerSecond, Coverage, Percent, Pid, ProcessKey, Uid};

use crate::apps::ProcessRole;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ProcessState {
    Running,
    Sleeping,
    /// Uninterruptible sleep, usually waiting on I/O.
    DiskSleep,
    Stopped,
    TracingStop,
    Zombie,
    Dead,
    Idle,
    #[default]
    Unknown,
}

impl ProcessState {
    pub fn from_code(code: u8) -> Self {
        match code {
            b'R' => Self::Running,
            b'S' => Self::Sleeping,
            b'D' => Self::DiskSleep,
            b'T' => Self::Stopped,
            b't' => Self::TracingStop,
            b'Z' => Self::Zombie,
            b'X' | b'x' => Self::Dead,
            b'I' => Self::Idle,
            _ => Self::Unknown,
        }
    }

    /// The one-letter code `ps` shows.
    pub const fn code(self) -> char {
        match self {
            Self::Running => 'R',
            Self::Sleeping => 'S',
            Self::DiskSleep => 'D',
            Self::Stopped => 'T',
            Self::TracingStop => 't',
            Self::Zombie => 'Z',
            Self::Dead => 'X',
            Self::Idle => 'I',
            Self::Unknown => '?',
        }
    }
}

/// Per-process figures measured by other collectors (GPU, network) and
/// attributed through [`super::ProcessTable::apply_external`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ExternalUsage {
    pub gpu: Percent,
    pub gpu_memory: Bytes,
    pub network_receive: BytesPerSecond,
    pub network_transmit: BytesPerSecond,
    /// TCP bytes since the engine first saw the process.
    pub network_received_total: Bytes,
    pub network_transmitted_total: Bytes,
}

impl std::ops::AddAssign for ExternalUsage {
    fn add_assign(&mut self, other: Self) {
        self.gpu += other.gpu;
        self.gpu_memory += other.gpu_memory;
        self.network_receive += other.network_receive;
        self.network_transmit += other.network_transmit;
        self.network_received_total += other.network_received_total;
        self.network_transmitted_total += other.network_transmitted_total;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProcessRow {
    pub key: ProcessKey,
    pub parent: Pid,
    pub uid: Uid,
    pub name: Arc<str>,
    pub command_line: Arc<str>,
    /// `None` for kernel threads and other users' processes.
    pub executable: Option<Arc<str>>,
    /// Index into [`super::ProcessTable::apps`].
    pub app: usize,
    pub role: ProcessRole,
    pub state: ProcessState,
    pub threads: u32,
    pub nice: i32,
    /// 100 % = one core.
    pub cpu: Percent,
    /// 100 % = every logical CPU.
    pub cpu_machine: Percent,
    /// User plus system time since start.
    pub cpu_time: Duration,
    pub resident: Bytes,
    /// PSS, refreshed round-robin; `None` until read or when not readable.
    pub proportional: Option<Bytes>,
    /// USS (private pages), with `proportional`.
    pub unique: Option<Bytes>,
    pub swap: Option<Bytes>,
    /// `VmHWM`, read for inspected and expanded rows only.
    pub peak_resident: Option<Bytes>,
    pub minor_faults: u64,
    pub major_faults: u64,
    pub disk_read: BytesPerSecond,
    pub disk_write: BytesPerSecond,
    /// Storage bytes since the process started (`/proc/<pid>/io`), known
    /// while I/O is sampled: on the Disk page and for the inspected process.
    pub read_since_start: Option<Bytes>,
    pub written_since_start: Option<Bytes>,
    pub external: ExternalUsage,
    pub started_unix: u64,
    /// Younger than ten seconds.
    pub is_new: bool,
    pub kernel_thread: bool,
    /// `OwnProcesses` when restricted figures (I/O, PSS) were not readable.
    pub coverage: Coverage,
}

impl ProcessRow {
    /// PSS when known, RSS otherwise.
    pub fn memory(&self) -> Bytes {
        self.proportional.unwrap_or(self.resident)
    }
}

/// A process that exited within the last ten minutes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndedProcess {
    pub key: ProcessKey,
    pub name: Arc<str>,
    pub app: AppKey,
    pub app_name: Arc<str>,
    pub started_unix: u64,
    pub ended_unix: u64,
    pub peak_memory: Bytes,
}
