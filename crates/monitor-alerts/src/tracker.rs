use std::sync::Arc;

use crate::alert::AlertKind;
use crate::input::{AlertInput, AppObservation};
use crate::rules::baseline::{Baseline, BurstRule};
use crate::rules::growth::GrowthWindow;
use crate::rules::minute::{AppMinute, AppMinuteSums, MachineMinuteSums};
use crate::rules::streak::Streak;
use crate::thresholds::Thresholds;

/// A rule that held for the closed minute, before cooldown and quieting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    pub kind: AlertKind,
    pub value: f64,
    pub limit: f64,
}

const APP_RULES: usize = 4;
const MACHINE_RULES: usize = 2;

/// Per-app rule state: a few dozen bytes, independent of how long it runs.
#[derive(Clone, Debug)]
pub struct AppTracker {
    pub name: Arc<str>,
    pub last_seen_minute_unix: i64,
    pub last_minute: Option<(i64, AppMinute)>,
    sums: AppMinuteSums,
    cpu: Streak,
    growth: GrowthWindow,
    disk: Baseline,
    network: Baseline,
}

impl AppTracker {
    pub fn new(name: Arc<str>, minute_unix: i64) -> Self {
        Self {
            name,
            last_seen_minute_unix: minute_unix,
            last_minute: None,
            sums: AppMinuteSums::default(),
            cpu: Streak::default(),
            growth: GrowthWindow::default(),
            disk: Baseline::default(),
            network: Baseline::default(),
        }
    }

    pub fn add(&mut self, app: &AppObservation, seconds: f64, minute_unix: i64) {
        self.sums.add(app, seconds);
        self.last_seen_minute_unix = minute_unix;
    }

    /// The finished minute's averages, if the app was seen during it.
    pub fn minute(&self, minute_unix: i64) -> Option<&AppMinute> {
        self.last_minute
            .as_ref()
            .filter(|(start, _)| *start == minute_unix)
            .map(|(_, minute)| minute)
    }

    pub fn close_minute(
        &mut self,
        minute_unix: i64,
        thresholds: &Thresholds,
    ) -> [Option<Candidate>; APP_RULES] {
        let Some(minute) = self.sums.take() else {
            return [None; APP_RULES];
        };
        self.last_minute = Some((minute_unix, minute));
        let cpu_limit = f64::from(thresholds.sustained_cpu.get());
        let sustained = self
            .cpu
            .update(minute_unix, minute.cpu_percent >= cpu_limit)
            >= thresholds.sustained_cpu_minutes.max(1);
        let growth_limit = thresholds.memory_growth.get() as f64;
        let growth = self
            .growth
            .update(
                minute_unix,
                minute.memory_bytes,
                thresholds.memory_growth_minutes,
            )
            .filter(|growth| *growth >= growth_limit);
        let burst_rule = |floor: f64| BurstRule {
            factor: thresholds.burst_factor,
            floor,
            warmup_minutes: thresholds.burst_warmup_minutes,
        };
        let disk_rule = burst_rule(thresholds.disk_burst_floor.get());
        let network_rule = burst_rule(thresholds.network_burst_floor.get());
        let disk_burst = self
            .disk
            .check_and_update(minute.disk_bytes_per_second, &disk_rule);
        let network_burst = self
            .network
            .check_and_update(minute.network_bytes_per_second, &network_rule);
        [
            sustained.then_some(Candidate {
                kind: AlertKind::SustainedCpu,
                value: minute.cpu_percent,
                limit: cpu_limit,
            }),
            growth.map(|growth| Candidate {
                kind: AlertKind::MemoryGrowth,
                value: growth,
                limit: growth_limit,
            }),
            disk_burst.then_some(Candidate {
                kind: AlertKind::DiskBurst,
                value: minute.disk_bytes_per_second,
                limit: disk_rule.floor,
            }),
            network_burst.then_some(Candidate {
                kind: AlertKind::NetworkBurst,
                value: minute.network_bytes_per_second,
                limit: network_rule.floor,
            }),
        ]
    }
}

#[derive(Clone, Debug, Default)]
pub struct MachineTracker {
    sums: MachineMinuteSums,
    hot: Streak,
    busy_gpu: Streak,
}

impl MachineTracker {
    pub fn add(&mut self, input: &AlertInput<'_>, seconds: f64) {
        self.sums.add(input, seconds);
    }

    pub fn close_minute(
        &mut self,
        minute_unix: i64,
        thresholds: &Thresholds,
    ) -> [Option<Candidate>; MACHINE_RULES] {
        let Some(minute) = self.sums.take() else {
            return [None; MACHINE_RULES];
        };
        let hot_limit = f64::from(thresholds.hot.get());
        let gpu_limit = f64::from(thresholds.busy_gpu.get());
        let hot = minute
            .hottest_celsius
            .is_some_and(|celsius| celsius >= hot_limit);
        let hot_minutes = self.hot.update(minute_unix, hot);
        let gpu_minutes = self
            .busy_gpu
            .update(minute_unix, minute.gpu_percent >= gpu_limit);
        [
            (hot_minutes >= thresholds.hot_minutes.max(1)).then(|| Candidate {
                kind: AlertKind::MachineHot,
                value: minute.hottest_celsius.unwrap_or(hot_limit),
                limit: hot_limit,
            }),
            (gpu_minutes >= thresholds.busy_gpu_minutes.max(1)).then_some(Candidate {
                kind: AlertKind::GpuBusy,
                value: minute.gpu_percent,
                limit: gpu_limit,
            }),
        ]
    }
}
