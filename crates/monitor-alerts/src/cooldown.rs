use std::collections::HashMap;

use monitor_core::AppKey;

use crate::alert::AlertKind;

/// When each `(app, kind)` may fire again. Entries expire, so the map only
/// holds alerts from the last cooldown period.
#[derive(Clone, Debug, Default)]
pub struct Cooldowns {
    ready_at: HashMap<(Option<AppKey>, AlertKind), i64>,
}

impl Cooldowns {
    /// Starts a cooldown and returns `true` unless one is still running.
    pub fn try_start(
        &mut self,
        app: Option<&AppKey>,
        kind: AlertKind,
        now_unix: i64,
        cooldown_seconds: i64,
    ) -> bool {
        let ready_at = self
            .ready_at
            .entry((app.cloned(), kind))
            .or_insert(i64::MIN);
        if now_unix < *ready_at {
            return false;
        }
        *ready_at = now_unix + cooldown_seconds;
        true
    }

    pub fn prune(&mut self, now_unix: i64) {
        self.ready_at.retain(|_, ready_at| now_unix < *ready_at);
    }
}
