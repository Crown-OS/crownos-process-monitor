//! CPU, memory, process table, app grouping, inspector details and process
//! actions for the CrownOS process monitor.

pub mod actions;
pub mod apps;
pub mod cpu;
pub mod inspect;
pub mod memory;
pub mod pressure;
pub mod process;

mod util;

pub use util::passwd::user_names;

pub use actions::{
    ActionError, AppAction, CpuMask, ProcessAction, RefuseReason, Verdict, affinity, app_verdict,
    perform, perform_app, processes_affected, renice, set_affinity, verdict,
};
pub use apps::{
    AppRow, AppSortKey, Direction, MemoryMethod, ProcessRole, ProcessSortKey, RoleBreakdown,
    sort_apps, sort_processes_of_app,
};
pub use cpu::{
    CacheKind, CacheLevel, CoreType, CpuCollector, CpuInfo, CpuPolicy, CpuSnapshot, CpuStates,
    CpuTopology, EventRates, FrequencySummary, LoadAverage, LogicalCpuUsage, SmtState,
    Vulnerability, VulnerabilityStatus, cpu_info,
};
pub use inspect::{EnvironmentExport, ProcessDetails, inspect};
pub use memory::{
    HugePages, MemoryCategories, MemoryCollector, MemoryModule, MemoryPressure, MemorySnapshot,
    PressureLevel, SwapDevice, SwapKind, ZramDevice, ZswapUsage, classify_pressure, memory_modules,
};
pub use pressure::PressureAverages;
pub use process::{
    EndedProcess, ExternalUsage, IoCounters, PrivilegedUsage, ProcessCollector, ProcessRow,
    ProcessState, ProcessTable, SmapsRollup, StatFields, parse_stat,
};
