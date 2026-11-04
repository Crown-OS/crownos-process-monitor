use std::sync::Arc;
use std::time::Duration;

use monitor_core::SourceResult;

/// The sources a snapshot is assembled from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Domain {
    Processes,
    Cpu,
    Memory,
    Disk,
    Network,
    Gpu,
    Sensors,
    Power,
    History,
}

impl Domain {
    pub const COUNT: usize = 9;
}

/// Wall time each domain's collector took on the last tick, plus the
/// shared descriptor walk (`/proc/<pid>/fd`) that GPU and network views need.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DomainCosts {
    domains: [Duration; Domain::COUNT],
    pub fd_index: Duration,
}

impl DomainCosts {
    pub fn get(&self, domain: Domain) -> Duration {
        self.domains[domain as usize]
    }

    pub(crate) fn set(&mut self, domain: Domain, cost: Duration) {
        self.domains[domain as usize] = cost;
    }
}

/// Which domains failed on the last tick, and why. A failure is never
/// fatal: the domain keeps its previous figures and the UI shows its empty state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Availability {
    failures: [Option<Arc<str>>; Domain::COUNT],
}

impl Availability {
    pub fn is_available(&self, domain: Domain) -> bool {
        self.failures[domain as usize].is_none()
    }

    pub fn failure(&self, domain: Domain) -> Option<&str> {
        self.failures[domain as usize].as_deref()
    }

    pub(crate) fn mark_unavailable(&mut self, domain: Domain, reason: impl ToString) {
        let failure = &mut self.failures[domain as usize];
        if failure.is_none() {
            *failure = Some(reason.to_string().into());
        }
    }

    pub(crate) fn record(&mut self, domain: Domain, outcome: SourceResult<()>) {
        match outcome {
            Ok(()) => self.failures[domain as usize] = None,
            Err(error) => self.mark_unavailable(domain, error),
        }
    }
}

#[cfg(test)]
mod tests {
    use monitor_core::SourceError;

    use super::*;

    #[test]
    fn keeps_the_first_reason_until_recovery() {
        let mut availability = Availability::default();
        availability.record(Domain::Gpu, Err(SourceError::Unavailable("GPU")));
        availability.record(Domain::Gpu, Err(SourceError::parse("fdinfo")));
        assert_eq!(
            availability.failure(Domain::Gpu),
            Some("GPU is unavailable")
        );
        assert!(availability.is_available(Domain::Cpu));
        availability.record(Domain::Gpu, Ok(()));
        assert!(availability.is_available(Domain::Gpu));
    }
}
