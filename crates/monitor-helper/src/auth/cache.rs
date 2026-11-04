use std::collections::HashMap;
use std::sync::Mutex;

use super::{AuthError, Authorizer, Decision};
use crate::{Caller, Capability};

/// Remembers granted read capabilities for the lifetime of a client's bus
/// connection, so figures fetched every second are authorized once.
/// Anything that changes the system is asked about every time.
pub struct CachedAuthorizer<A> {
    inner: A,
    granted: Mutex<HashMap<String, Vec<Capability>>>,
}

impl<A> CachedAuthorizer<A> {
    pub fn new(inner: A) -> Self {
        Self {
            inner,
            granted: Mutex::new(HashMap::new()),
        }
    }

    fn remembered(&self, caller: &Caller, capability: Capability) -> bool {
        self.granted.lock().is_ok_and(|granted| {
            granted
                .get(caller.bus_name.as_str())
                .is_some_and(|capabilities| capabilities.contains(&capability))
        })
    }

    fn remember(&self, caller: &Caller, capability: Capability) {
        if let Ok(mut granted) = self.granted.lock() {
            let capabilities = granted.entry(caller.bus_name.to_string()).or_default();
            if !capabilities.contains(&capability) {
                capabilities.push(capability);
            }
        }
    }
}

impl<A: Authorizer> Authorizer for CachedAuthorizer<A> {
    async fn authorize(
        &self,
        caller: &Caller,
        capability: Capability,
    ) -> Result<Decision, AuthError> {
        if capability.is_read_only() && self.remembered(caller, capability) {
            return Ok(Decision::Allowed);
        }
        let decision = self.inner.authorize(caller, capability).await?;
        if decision.is_allowed() && capability.is_read_only() {
            self.remember(caller, capability);
        }
        Ok(decision)
    }

    fn forget(&self, caller_bus_name: &str) {
        if let Ok(mut granted) = self.granted.lock() {
            granted.remove(caller_bus_name);
        }
        self.inner.forget(caller_bus_name);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use zbus::names::OwnedUniqueName;

    use super::*;

    struct CountingAuthorizer {
        calls: AtomicUsize,
    }

    impl Authorizer for CountingAuthorizer {
        async fn authorize(
            &self,
            _caller: &Caller,
            _capability: Capability,
        ) -> Result<Decision, AuthError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(Decision::Allowed)
        }
    }

    fn caller(name: &str) -> Option<Caller> {
        Some(Caller {
            bus_name: OwnedUniqueName::try_from(name).ok()?,
            uid: 1000,
            pid: std::process::id(),
            start_ticks: 1,
            supplementary_groups: Vec::new(),
            pidfd: None,
        })
    }

    fn calls(cache: &CachedAuthorizer<CountingAuthorizer>) -> usize {
        cache.inner.calls.load(Ordering::Relaxed)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn caches_reads_but_not_writes() {
        let cache = CachedAuthorizer::new(CountingAuthorizer {
            calls: AtomicUsize::new(0),
        });
        let Some(client) = caller(":1.42") else {
            return;
        };
        for _ in 0..3 {
            assert!(
                cache
                    .authorize(&client, Capability::ReadEnergy)
                    .await
                    .is_ok()
            );
        }
        assert_eq!(calls(&cache), 1);
        for _ in 0..2 {
            assert!(
                cache
                    .authorize(&client, Capability::ControlFans)
                    .await
                    .is_ok()
            );
        }
        assert_eq!(calls(&cache), 3);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn forgets_departed_clients() {
        let cache = CachedAuthorizer::new(CountingAuthorizer {
            calls: AtomicUsize::new(0),
        });
        let Some(client) = caller(":1.7") else { return };
        assert!(
            cache
                .authorize(&client, Capability::ReadEnergy)
                .await
                .is_ok()
        );
        cache.forget(":1.7");
        assert!(
            cache
                .authorize(&client, Capability::ReadEnergy)
                .await
                .is_ok()
        );
        assert_eq!(calls(&cache), 2);
    }
}
