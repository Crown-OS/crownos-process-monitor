use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use monitor_core::BytesPerSecond;
use monitor_helper::CgroupTraffic;
use monitor_system::AppRow;

/// Per-cgroup byte rates from the helper's eBPF counters, joined to apps
/// through the cgroup each app owns.
#[derive(Default)]
pub(crate) struct CgroupAccounting {
    previous: HashMap<u64, (u64, u64)>,
    traffic: Arc<Vec<CgroupTraffic>>,
    rates: Vec<(BytesPerSecond, BytesPerSecond)>,
    observed_at: Option<Duration>,
}

impl CgroupAccounting {
    pub fn update(&mut self, traffic: Arc<Vec<CgroupTraffic>>, now: Duration) {
        let seconds = self
            .observed_at
            .replace(now)
            .map_or(0.0, |earlier| now.saturating_sub(earlier).as_secs_f64());
        self.rates.clear();
        for cgroup in traffic.iter() {
            let counters = (cgroup.received_bytes, cgroup.transmitted_bytes);
            let rates = match self.previous.insert(cgroup.cgroup_id, counters) {
                Some((received, transmitted)) if seconds > 0.0 => (
                    BytesPerSecond::between(received, counters.0, seconds),
                    BytesPerSecond::between(transmitted, counters.1, seconds),
                ),
                _ => (BytesPerSecond::ZERO, BytesPerSecond::ZERO),
            };
            self.rates.push(rates);
        }
        if self.previous.len() > traffic.len() {
            self.previous
                .retain(|id, _| traffic.iter().any(|cgroup| cgroup.cgroup_id == *id));
        }
        self.traffic = traffic;
    }

    pub fn is_measuring(&self) -> bool {
        !self.rates.is_empty()
    }

    /// Replaces the TCP-only figures of every app that owns a cgroup with
    /// the all-protocol traffic of that cgroup and its children.
    pub fn apply(&self, apps: &mut [AppRow]) {
        for app in apps {
            let Some(scope) = app.scope.as_deref() else {
                continue;
            };
            let scope = scope.trim_start_matches('/');
            let (receive, transmit) = self
                .traffic
                .iter()
                .zip(&self.rates)
                .filter(|(cgroup, _)| within(&cgroup.cgroup_path, scope))
                .fold(
                    (BytesPerSecond::ZERO, BytesPerSecond::ZERO),
                    |(receive, transmit), (_, rates)| (receive + rates.0, transmit + rates.1),
                );
            app.external.network_receive = receive;
            app.external.network_transmit = transmit;
        }
    }
}

fn within(path: &str, scope: &str) -> bool {
    path.strip_prefix(scope)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn traffic(id: u64, path: &str, received: u64) -> CgroupTraffic {
        CgroupTraffic {
            cgroup_id: id,
            cgroup_path: path.into(),
            received_bytes: received,
            transmitted_bytes: 0,
        }
    }

    #[test]
    fn matches_scopes_and_children_only() {
        assert!(within("app.slice/a.scope", "app.slice/a.scope"));
        assert!(within("app.slice/a.scope/sub", "app.slice/a.scope"));
        assert!(!within("app.slice/a.scope2", "app.slice/a.scope"));
    }

    #[test]
    fn turns_counters_into_rates() {
        let mut accounting = CgroupAccounting::default();
        accounting.update(
            Arc::new(vec![traffic(1, "a.scope", 100)]),
            Duration::from_secs(1),
        );
        accounting.update(
            Arc::new(vec![
                traffic(1, "a.scope", 1100),
                traffic(2, "a.scope/child", 0),
            ]),
            Duration::from_secs(3),
        );
        assert_eq!(accounting.rates[0].0, BytesPerSecond::new(500.0));
        assert_eq!(accounting.rates[1].0, BytesPerSecond::ZERO);
        assert!(accounting.is_measuring());
    }
}
