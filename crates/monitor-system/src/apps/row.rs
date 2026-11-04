use std::sync::Arc;

use monitor_core::{AppKey, AppKind, Bytes, BytesPerSecond, Coverage, Percent};

use super::role::RoleBreakdown;
use crate::process::ExternalUsage;

/// How an app's memory figure was measured, best first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MemoryMethod {
    /// The app's own cgroup `memory.current`: exact.
    Cgroup,
    /// Σ PSS of its processes: shared pages split fairly.
    Pss,
    /// Σ RSS: approximate, shared pages counted once per process.
    #[default]
    Rss,
}

/// One app in the table, aggregated over its processes this tick.
#[derive(Clone, Debug, PartialEq)]
pub struct AppRow {
    pub key: AppKey,
    pub name: Arc<str>,
    /// Icon name from the desktop entry, for the icon theme lookup.
    pub icon: Option<Arc<str>>,
    pub kind: AppKind,
    pub process_count: u32,
    pub threads: u32,
    /// 100 % = one core.
    pub cpu: Percent,
    /// 100 % = every logical CPU.
    pub cpu_machine: Percent,
    pub memory: Bytes,
    pub memory_method: MemoryMethod,
    pub resident: Bytes,
    pub disk_read: BytesPerSecond,
    pub disk_write: BytesPerSecond,
    pub external: ExternalUsage,
    pub roles: RoleBreakdown,
    /// The app's own cgroup (relative to `/sys/fs/cgroup`) when every member
    /// lives in it; app-wide actions use it.
    pub scope: Option<Arc<str>>,
    /// The scope is frozen (`cgroup.events` reads `frozen 1`): Resume applies,
    /// Pause does not. Always `false` without a scope.
    pub frozen: bool,
    /// Earliest member start, unix seconds.
    pub started_unix: u64,
    pub coverage: Coverage,
}
