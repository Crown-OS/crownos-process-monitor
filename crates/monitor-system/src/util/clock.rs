use std::time::Duration;

use monitor_core::util::time::wall_unix_seconds;

use rustix::time::{ClockId, clock_gettime};

const MILLIS_PER_SECOND: u64 = 1000;

pub(crate) fn clock_ticks_per_second() -> u64 {
    rustix::param::clock_ticks_per_second().max(1)
}

pub(crate) fn page_size() -> u64 {
    rustix::param::page_size() as u64
}

pub(crate) fn ticks_to_duration(ticks: u64, ticks_per_second: u64) -> Duration {
    Duration::from_millis(ticks.saturating_mul(MILLIS_PER_SECOND) / ticks_per_second.max(1))
}

/// Time since boot including suspend, the clock `/proc/<pid>/stat` start times use.
pub(crate) fn since_boot() -> Duration {
    let spec = clock_gettime(ClockId::Boottime);
    Duration::new(
        u64::try_from(spec.tv_sec).unwrap_or(0),
        u32::try_from(spec.tv_nsec).unwrap_or(0),
    )
}

/// Wall-clock start of a process whose stat field 22 is `start_ticks`.
pub(crate) fn start_unix_seconds(start_ticks: u64, ticks_per_second: u64) -> u64 {
    let started_after_boot = ticks_to_duration(start_ticks, ticks_per_second);
    let age = since_boot().saturating_sub(started_after_boot);
    wall_unix_seconds().saturating_sub(age.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_ticks() {
        assert_eq!(ticks_to_duration(250, 100), Duration::from_millis(2500));
        assert_eq!(ticks_to_duration(1, 0), Duration::from_secs(1));
    }

    #[test]
    fn own_start_is_in_the_past() {
        let started = start_unix_seconds(0, clock_ticks_per_second());
        assert!(started <= wall_unix_seconds());
    }
}
