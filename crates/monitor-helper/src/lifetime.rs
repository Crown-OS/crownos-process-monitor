//! The helper runs only while someone uses it: it exits once the last
//! client left the bus and nobody called for [`IDLE_GRACE`].

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tokio::sync::Notify;

use crate::util::lock::lock;

pub const IDLE_GRACE: Duration = Duration::from_secs(30);

/// Which bus clients called the helper, and since when nobody has.
#[derive(Debug)]
pub struct IdleClock {
    clients: HashSet<String>,
    idle_since: Option<Instant>,
}

impl IdleClock {
    /// Starts idle: an activation nobody follows up on ends after the grace.
    pub fn new(now: Instant) -> Self {
        Self {
            clients: HashSet::new(),
            idle_since: Some(now),
        }
    }

    /// Returns `true` for a client not seen before.
    pub fn arrived(&mut self, bus_name: &str) -> bool {
        self.idle_since = None;
        !self.clients.contains(bus_name) && self.clients.insert(bus_name.to_owned())
    }

    /// Returns `true` when `bus_name` was a client.
    pub fn departed(&mut self, bus_name: &str, now: Instant) -> bool {
        let was_client = self.clients.remove(bus_name);
        if self.clients.is_empty() && self.idle_since.is_none() {
            self.idle_since = Some(now);
        }
        was_client
    }

    pub fn is_client(&self, bus_name: &str) -> bool {
        self.clients.contains(bus_name)
    }

    pub fn exit_deadline(&self) -> Option<Instant> {
        self.idle_since.map(|since| since + IDLE_GRACE)
    }

    pub fn should_exit(&self, now: Instant) -> bool {
        self.exit_deadline().is_some_and(|deadline| now >= deadline)
    }
}

/// [`IdleClock`] shared between method handlers and the exit loop, which
/// is woken whenever a new client arrives.
#[derive(Debug)]
pub struct ClientRegistry {
    clock: Mutex<IdleClock>,
    arrivals: Notify,
}

impl Default for ClientRegistry {
    fn default() -> Self {
        Self {
            clock: Mutex::new(IdleClock::new(Instant::now())),
            arrivals: Notify::new(),
        }
    }
}

impl ClientRegistry {
    pub fn arrived(&self, bus_name: &str) {
        if lock(&self.clock).arrived(bus_name) {
            self.arrivals.notify_one();
        }
    }

    pub fn departed(&self, bus_name: &str) -> bool {
        lock(&self.clock).departed(bus_name, Instant::now())
    }

    pub fn is_client(&self, bus_name: &str) -> bool {
        lock(&self.clock).is_client(bus_name)
    }

    pub fn exit_deadline(&self) -> Option<Instant> {
        lock(&self.clock).exit_deadline()
    }

    pub fn should_exit(&self) -> bool {
        lock(&self.clock).should_exit(Instant::now())
    }

    /// Resolves after a client arrived that was not known before.
    pub async fn arrival(&self) {
        self.arrivals.notified().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unused_activation_exits_after_grace() {
        let start = Instant::now();
        let clock = IdleClock::new(start);
        assert!(!clock.should_exit(start + IDLE_GRACE - Duration::from_secs(1)));
        assert!(clock.should_exit(start + IDLE_GRACE));
    }

    #[test]
    fn clients_hold_the_helper_until_the_last_leaves() {
        let start = Instant::now();
        let later = |seconds| start + Duration::from_secs(seconds);
        let mut clock = IdleClock::new(start);
        assert!(clock.arrived(":1.1"));
        assert!(!clock.arrived(":1.1"), "a known client is not new");
        assert!(clock.arrived(":1.2"));
        assert_eq!(clock.exit_deadline(), None);
        assert!(!clock.should_exit(later(3600)));

        assert!(clock.departed(":1.1", later(10)));
        assert!(!clock.departed(":1.9", later(10)), "strangers are ignored");
        assert_eq!(clock.exit_deadline(), None, "one client remains");

        assert!(clock.departed(":1.2", later(20)));
        assert_eq!(clock.exit_deadline(), Some(later(20) + IDLE_GRACE));
        assert!(!clock.should_exit(later(49)));
        assert!(clock.should_exit(later(50)));
    }

    #[test]
    fn a_returning_client_cancels_the_countdown() {
        let start = Instant::now();
        let mut clock = IdleClock::new(start);
        clock.arrived(":1.1");
        clock.departed(":1.1", start);
        assert!(clock.exit_deadline().is_some());
        clock.arrived(":1.3");
        assert_eq!(clock.exit_deadline(), None);
        assert!(clock.is_client(":1.3"));
        assert!(!clock.is_client(":1.1"));
    }

    #[test]
    fn stranger_departures_do_not_restart_the_countdown() {
        let start = Instant::now();
        let mut clock = IdleClock::new(start);
        clock.departed(":1.5", start + Duration::from_secs(25));
        assert_eq!(clock.exit_deadline(), Some(start + IDLE_GRACE));
    }
}
