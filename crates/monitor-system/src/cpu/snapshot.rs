use std::time::Duration;

use monitor_core::{Megahertz, Percent};

use super::policy::CpuPolicy;
use crate::pressure::PressureAverages;

/// One CPU sample. Percentages are of the whole machine unless noted.
#[derive(Clone, Debug, Default)]
pub struct CpuSnapshot {
    /// The interval spans a suspend or is the first sample: rates are zero,
    /// record a gap.
    pub gap: bool,
    pub total: Percent,
    pub states: CpuStates,
    /// Online logical CPUs in id order. `usage` is of that CPU.
    pub logical: Vec<LogicalCpuUsage>,
    /// Filled only while the CPU page is visible.
    pub frequency: FrequencySummary,
    pub policy: CpuPolicy,
    pub load: LoadAverage,
    /// `None` when the kernel has no PSI.
    pub pressure: Option<PressureAverages>,
    pub events: EventRates,
    pub procs_running: u32,
    pub procs_blocked: u32,
    /// Every scheduling entity (threads, kernel threads included).
    pub threads: u32,
    pub uptime: Duration,
}

/// Time-state shares that stack to 100 % with `idle`. `guest` is taken out
/// of `user` and `nice`, so nothing is counted twice.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CpuStates {
    pub user: Percent,
    pub nice: Percent,
    pub system: Percent,
    pub idle: Percent,
    pub iowait: Percent,
    pub irq: Percent,
    pub softirq: Percent,
    pub steal: Percent,
    pub guest: Percent,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LogicalCpuUsage {
    pub cpu: u32,
    pub usage: Percent,
    pub frequency: Option<Megahertz>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrequencySummary {
    pub average: Option<Megahertz>,
    pub min: Option<Megahertz>,
    pub max: Option<Megahertz>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LoadAverage {
    pub one: f32,
    pub five: f32,
    pub fifteen: f32,
}

/// Kernel event rates, per second.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EventRates {
    pub context_switches: f32,
    pub interrupts: f32,
    pub soft_interrupts: f32,
    pub forks: f32,
}
