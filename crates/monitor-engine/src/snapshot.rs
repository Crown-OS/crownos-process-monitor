use std::sync::Arc;
use std::time::Duration;

use monitor_alerts::{QuietRule, StrainState, WorthALook};
use monitor_core::{Elapsed, Interest, Percent, Tick};
use monitor_hardware::{GpuSnapshot, PowerSnapshot, SensorSnapshot, SystemDraw};
use monitor_io::{DiskSnapshot, NetworkSnapshot};
use monitor_system::{CpuInfo, CpuSnapshot, MemoryModule, MemorySnapshot, ProcessTable};

use crate::recording::max_percent;
use crate::{AppEnergy, Availability, DomainCosts, EnergyEstimate, HelperStatus};

/// Everything measured in one tick. Published as `Arc<Snapshot>`; static
/// and rarely changing parts are `Arc`s, so a clone shares them.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    /// Ticks published since start; 0 before the first sample.
    pub sequence: u64,
    /// `None` before the first sample.
    pub tick: Option<Tick>,
    /// Since the previous tick. When `suspended`, record a gap.
    pub elapsed: Elapsed,
    /// What the UI was looking at when this tick was taken.
    pub interest: Interest,
    /// Collection, joins, history and alerts of this tick, publishing excluded.
    pub tick_cost: Duration,
    /// The collection part of `tick_cost`, per domain.
    pub costs: DomainCosts,
    pub processes: ProcessTable,
    pub cpu: CpuSnapshot,
    pub memory: MemorySnapshot,
    pub disk: DiskSnapshot,
    pub network: NetworkSnapshot,
    pub gpu: GpuSnapshot,
    pub sensors: SensorSnapshot,
    pub power: PowerSnapshot,
    /// Battery discharge or RAPL package plus discrete GPUs.
    pub system_draw: Option<SystemDraw>,
    /// Read once at start.
    pub cpu_info: Arc<CpuInfo>,
    /// Read once at start.
    pub memory_modules: Arc<[MemoryModule]>,
    /// The Worth a Look feed, past sessions included.
    pub alerts: Arc<WorthALook>,
    pub strain: StrainState,
    /// Active quiet rules; persisted and reloaded at start.
    pub quiet_rules: Arc<[QuietRule]>,
    pub availability: Availability,
    /// The privileged helper's availability; `Absent` without it.
    pub helper: HelperStatus,
    /// Power by App for this tick, parallel to `processes.apps`.
    pub energy: EnergyEstimate,
}

impl Snapshot {
    /// The estimated energy of the app at `app` (an index into
    /// `processes.apps`), when an energy source exists.
    pub fn app_energy(&self, app: usize) -> Option<AppEnergy> {
        self.energy.apps.get(app).copied()
    }

    /// The busiest GPU's utilization; zero without GPUs.
    pub fn busiest_gpu(&self) -> Percent {
        self.gpu
            .devices
            .iter()
            .filter_map(|device| device.metrics.utilization)
            .fold(Percent::ZERO, max_percent)
    }
}
