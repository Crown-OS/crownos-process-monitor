use crate::input::{AlertInput, AppObservation};

/// One app's averages over a finished minute.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AppMinute {
    pub cpu_percent: f64,
    pub memory_bytes: f64,
    pub disk_bytes_per_second: f64,
    pub network_bytes_per_second: f64,
    pub gpu_percent: f64,
}

/// Time-weighted sums for the minute in progress.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AppMinuteSums {
    seconds: f64,
    totals: AppMinute,
}

impl AppMinuteSums {
    pub fn add(&mut self, app: &AppObservation, seconds: f64) {
        self.seconds += seconds;
        let totals = &mut self.totals;
        totals.cpu_percent += f64::from(app.cpu_percent.get()) * seconds;
        totals.memory_bytes += app.memory.get() as f64 * seconds;
        totals.disk_bytes_per_second += app.disk_bytes_per_second.get() * seconds;
        totals.network_bytes_per_second += app.network_bytes_per_second.get() * seconds;
        totals.gpu_percent += f64::from(app.gpu_percent.get()) * seconds;
    }

    pub fn take(&mut self) -> Option<AppMinute> {
        let Self { seconds, totals } = std::mem::take(self);
        (seconds > 0.0).then(|| AppMinute {
            cpu_percent: totals.cpu_percent / seconds,
            memory_bytes: totals.memory_bytes / seconds,
            disk_bytes_per_second: totals.disk_bytes_per_second / seconds,
            network_bytes_per_second: totals.network_bytes_per_second / seconds,
            gpu_percent: totals.gpu_percent / seconds,
        })
    }
}

/// Machine averages over a finished minute.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MachineMinute {
    pub gpu_percent: f64,
    pub hottest_celsius: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MachineMinuteSums {
    seconds: f64,
    gpu_percent: f64,
    celsius_seconds: f64,
    celsius: f64,
}

impl MachineMinuteSums {
    pub fn add(&mut self, input: &AlertInput<'_>, seconds: f64) {
        self.seconds += seconds;
        self.gpu_percent += f64::from(input.gpu_percent.get()) * seconds;
        if let Some(celsius) = input.hottest_celsius {
            self.celsius_seconds += seconds;
            self.celsius += f64::from(celsius.get()) * seconds;
        }
    }

    pub fn take(&mut self) -> Option<MachineMinute> {
        let sums = std::mem::take(self);
        (sums.seconds > 0.0).then(|| MachineMinute {
            gpu_percent: sums.gpu_percent / sums.seconds,
            hottest_celsius: (sums.celsius_seconds > 0.0)
                .then(|| sums.celsius / sums.celsius_seconds),
        })
    }
}
