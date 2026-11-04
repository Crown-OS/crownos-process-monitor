use std::time::Duration;

use rustix::time::{ClockId, Timespec, clock_gettime};

/// If the boot clock advanced this much more than the monotonic clock, the
/// machine was suspended in between.
const SUSPEND_THRESHOLD: Duration = Duration::from_secs(1);

/// One sampling instant on every clock the engine cares about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tick {
    monotonic: Duration,
    boottime: Duration,
    wall_unix_seconds: u64,
}

/// Time between two ticks. Rates are computed with `seconds` (monotonic, so
/// suspend is excluded); `suspended` tells consumers to record a gap.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Elapsed {
    pub seconds: f64,
    pub suspended: bool,
}

impl Tick {
    #[must_use]
    pub fn now() -> Self {
        let wall_unix_seconds = crate::util::time::wall_unix_seconds();
        Self {
            monotonic: to_duration(clock_gettime(ClockId::Monotonic)),
            boottime: to_duration(clock_gettime(ClockId::Boottime)),
            wall_unix_seconds,
        }
    }

    #[must_use]
    pub const fn monotonic(&self) -> Duration {
        self.monotonic
    }

    /// Time since boot including suspend, the clock `/proc/<pid>/stat`
    /// start times are measured on.
    #[must_use]
    pub const fn boottime(&self) -> Duration {
        self.boottime
    }

    #[must_use]
    pub const fn wall_unix_seconds(&self) -> u64 {
        self.wall_unix_seconds
    }

    #[must_use]
    pub fn since(&self, earlier: &Self) -> Elapsed {
        let monotonic = self.monotonic.saturating_sub(earlier.monotonic);
        let boottime = self.boottime.saturating_sub(earlier.boottime);
        Elapsed {
            seconds: monotonic.as_secs_f64(),
            suspended: boottime.saturating_sub(monotonic) > SUSPEND_THRESHOLD,
        }
    }
}

fn to_duration(spec: Timespec) -> Duration {
    let seconds = u64::try_from(spec.tv_sec).unwrap_or(0);
    let nanos = u32::try_from(spec.tv_nsec).unwrap_or(0);
    Duration::new(seconds, nanos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_suspend_gap() {
        let before = Tick {
            monotonic: Duration::from_secs(10),
            boottime: Duration::from_secs(10),
            wall_unix_seconds: 0,
        };
        let awake = Tick {
            monotonic: Duration::from_secs(11),
            boottime: Duration::from_secs(11),
            wall_unix_seconds: 1,
        };
        let resumed = Tick {
            monotonic: Duration::from_secs(12),
            boottime: Duration::from_secs(600),
            wall_unix_seconds: 600,
        };
        assert!(!awake.since(&before).suspended);
        assert!(resumed.since(&awake).suspended);
        assert_eq!(resumed.since(&awake).seconds, 1.0);
    }

    #[test]
    fn now_is_monotonic() {
        let first = Tick::now();
        let second = Tick::now();
        assert!(second.monotonic() >= first.monotonic());
    }
}
