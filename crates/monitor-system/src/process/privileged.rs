use std::time::Duration;

use monitor_core::{Bytes, BytesPerSecond, ProcessKey};

use super::entry::{IoState, ProcessEntry};
use super::files::{IoCounters, SmapsRollup};

/// Ticks a helper reading stays valid without a newer one.
const FRESH_TICKS: u64 = 3;

/// One process as measured by the privileged helper, for figures this user
/// cannot read (other users' I/O and PSS).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrivilegedUsage {
    pub key: ProcessKey,
    pub io: IoCounters,
    /// `None` until the helper measured it.
    pub rollup: Option<SmapsRollup>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PrivilegedFigures {
    io: IoCounters,
    observed_at: Duration,
    pub read: BytesPerSecond,
    pub write: BytesPerSecond,
    pub has_rollup: bool,
    generation: u64,
}

impl ProcessEntry {
    pub(crate) fn apply_privileged(
        &mut self,
        usage: &PrivilegedUsage,
        observed_at: Duration,
        generation: u64,
        rollup_cycle: u64,
    ) {
        let previous = self.privileged.take();
        let (read, write) = match previous {
            Some(earlier) if observed_at > earlier.observed_at => {
                let seconds = (observed_at - earlier.observed_at).as_secs_f64();
                (
                    BytesPerSecond::between(earlier.io.read_bytes, usage.io.read_bytes, seconds),
                    BytesPerSecond::between(earlier.io.write_bytes, usage.io.write_bytes, seconds),
                )
            }
            Some(earlier) => (earlier.read, earlier.write),
            None => (BytesPerSecond::ZERO, BytesPerSecond::ZERO),
        };
        let has_rollup = match usage.rollup {
            Some(rollup) if self.rollup_denied => {
                self.rollup = Some(rollup);
                self.rollup_cycle = rollup_cycle;
                true
            }
            _ => previous.is_some_and(|earlier| earlier.has_rollup),
        };
        self.privileged = Some(PrivilegedFigures {
            io: usage.io,
            observed_at,
            read,
            write,
            has_rollup,
            generation,
        });
    }

    pub(crate) fn expire_privileged(&mut self, generation: u64) {
        if self
            .privileged
            .is_some_and(|figures| figures.generation + FRESH_TICKS < generation)
        {
            self.privileged = None;
        }
    }

    /// Own `io` readings win; the helper fills in where they were denied.
    pub(crate) fn privileged_io(&self) -> Option<(BytesPerSecond, BytesPerSecond)> {
        match self.io {
            IoState::Open { .. } => None,
            IoState::Untried | IoState::Denied => {
                self.privileged.map(|figures| (figures.read, figures.write))
            }
        }
    }

    /// Bytes read from and written to storage since the process started, from
    /// its own `io` while I/O is sampled, else from the helper.
    pub(crate) fn io_totals(&self) -> Option<(Bytes, Bytes)> {
        let counters = match &self.io {
            IoState::Open { previous, .. } => *previous,
            IoState::Untried | IoState::Denied => self.privileged.map(|figures| figures.io),
        }?;
        Some((
            Bytes::new(counters.read_bytes),
            Bytes::new(counters.write_bytes),
        ))
    }

    /// Whether some figure (I/O or PSS) is unreadable even with the helper.
    pub(crate) fn is_restricted(&self) -> bool {
        self.io_restricted() || self.memory_restricted()
    }

    pub(crate) fn memory_restricted(&self) -> bool {
        self.rollup_denied && !self.privileged.is_some_and(|figures| figures.has_rollup)
    }

    pub(crate) fn io_restricted(&self) -> bool {
        matches!(self.io, IoState::Denied) && self.privileged.is_none()
    }
}
