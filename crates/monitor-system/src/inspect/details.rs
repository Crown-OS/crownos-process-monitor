use std::path::PathBuf;
use std::time::Duration;

use monitor_core::{AppKey, Bytes, Coverage, Pid, ProcessKey, Uid};

use super::elf::ElfInfo;
use super::security::{CapabilitySet, Emulation, Sandbox, SeccompMode};
use crate::apps::ProcessRole;
use crate::process::ProcessState;

/// Everything the inspector drawer shows for one process (plan §1.8 without
/// package provenance). Fields that need privileges are `None` with the
/// section's `coverage` set to [`Coverage::OwnProcesses`].
#[derive(Clone, Debug, PartialEq)]
pub struct ProcessDetails {
    pub identity: Identity,
    pub executable: Executable,
    pub command_line: Vec<String>,
    pub working_directory: Option<String>,
    pub security: Security,
    pub startup: Startup,
    pub desktop_entry: Option<DesktopEntryDetails>,
    pub resources: Resources,
    pub children: Vec<ChildProcess>,
    /// Own processes only. Never log it; exports leave it out unless asked.
    pub environment: Option<Vec<(String, String)>>,
    /// `OwnProcesses` when any section was cut short by permissions.
    pub coverage: Coverage,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Identity {
    pub key: ProcessKey,
    pub parent: Pid,
    pub name: String,
    pub app: AppKey,
    pub app_name: String,
    pub role: ProcessRole,
    pub kernel_thread: bool,
    pub uid: Uid,
    pub user: Option<String>,
    pub gid: u32,
    pub state: ProcessState,
    pub started_unix: u64,
    pub running_for: Duration,
    /// Controlling terminal, e.g. `pts/3`.
    pub tty: Option<String>,
    pub session: u32,
    pub process_group: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Executable {
    pub path: Option<String>,
    /// The file was replaced or removed on disk; a restart picks up the update.
    pub deleted: bool,
    pub elf: Option<ElfInfo>,
    pub emulation: Option<Emulation>,
    pub coverage: Coverage,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Security {
    pub sandbox: Sandbox,
    /// Namespace kinds (`mnt`, `net`, `pid`…) that differ from PID 1's.
    pub namespaces: Vec<&'static str>,
    pub seccomp: SeccompMode,
    pub no_new_privileges: bool,
    pub effective: CapabilitySet,
    pub permitted: CapabilitySet,
    pub bounding: CapabilitySet,
    /// AppArmor or SELinux label.
    pub lsm_label: Option<String>,
    pub coverage: Coverage,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Startup {
    /// systemd unit from the cgroup, e.g. `app-orca-2258.scope`.
    pub unit: Option<String>,
    pub unit_file: Option<PathBuf>,
    pub description: Option<String>,
    /// An autostart `.desktop` file that launches this program.
    pub autostart: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DesktopEntryDetails {
    pub id: String,
    pub path: PathBuf,
    pub name: Option<String>,
    pub generic_name: Option<String>,
    pub icon: Option<String>,
    pub categories: Option<String>,
    pub exec: Option<String>,
    pub startup_wm_class: Option<String>,
    /// The whole file, for the foldable view.
    pub text: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SchedulingPolicy {
    #[default]
    Normal,
    Fifo,
    RoundRobin,
    Batch,
    Idle,
    Deadline,
    Unknown(u32),
}

impl SchedulingPolicy {
    pub fn from_raw(policy: u32) -> Self {
        match policy {
            0 => Self::Normal,
            1 => Self::Fifo,
            2 => Self::RoundRobin,
            3 => Self::Batch,
            5 => Self::Idle,
            6 => Self::Deadline,
            other => Self::Unknown(other),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Normal => "SCHED_OTHER",
            Self::Fifo => "SCHED_FIFO",
            Self::RoundRobin => "SCHED_RR",
            Self::Batch => "SCHED_BATCH",
            Self::Idle => "SCHED_IDLE",
            Self::Deadline => "SCHED_DEADLINE",
            Self::Unknown(_) => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IoTotals {
    /// Bytes fetched from storage.
    pub read_bytes: u64,
    /// Bytes sent to storage.
    pub write_bytes: u64,
    /// Bytes passed through read-like syscalls, cache hits included.
    pub read_chars: u64,
    pub write_chars: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Resources {
    pub user_time: Duration,
    pub system_time: Duration,
    pub children_user_time: Duration,
    pub children_system_time: Duration,
    pub resident: Bytes,
    pub proportional: Option<Bytes>,
    pub unique: Option<Bytes>,
    pub swap: Option<Bytes>,
    pub peak_resident: Option<Bytes>,
    pub threads: u32,
    pub open_files: Option<u32>,
    pub minor_faults: u64,
    pub major_faults: u64,
    pub io: Option<IoTotals>,
    pub voluntary_switches: Option<u64>,
    pub involuntary_switches: Option<u64>,
    pub nice: i32,
    pub priority: i32,
    pub policy: SchedulingPolicy,
    /// `Cpus_allowed_list`, e.g. `0-15`.
    pub cpus_allowed: Option<String>,
    pub oom_score: Option<i32>,
    pub oom_score_adjust: Option<i32>,
    pub coverage: Coverage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChildProcess {
    pub key: ProcessKey,
    pub name: String,
}
