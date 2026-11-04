use monitor_core::Pid;

use super::{INIT_PID, MAX_ANCESTRY_DEPTH, ProcessCollector};
use crate::apps::resolver::AppIdentity;

impl ProcessCollector {
    pub(super) fn resolve_apps(&mut self) {
        for index in 0..self.row_slots.len() {
            let slot = self.row_slots[index];
            if self.entries[slot]
                .as_ref()
                .is_none_or(|entry| entry.app.is_some())
            {
                continue;
            }
            let Some((identity, owns_scope)) = self.resolve(slot) else {
                continue;
            };
            let app = self.registry.intern(identity);
            if let Some(entry) = self.entries[slot].as_mut() {
                entry.app = Some(app);
                entry.scope_path = None;
                if owns_scope {
                    entry.claim_scope();
                }
            }
        }
    }

    /// Plan §3.2: direct identity, else the nearest ancestor with an
    /// inheritable app or a desktop entry, else the process name.
    fn resolve(&mut self, slot: usize) -> Option<(AppIdentity, bool)> {
        let entries = &self.entries;
        let entry = entries[slot].as_ref()?;
        let facts = entry.facts();
        if let Some(resolved) = self.resolver.direct(&facts) {
            return Some((resolved.identity, resolved.owns_scope));
        }
        let mut cursor = entry.stat.parent;
        for _ in 0..MAX_ANCESTRY_DEPTH {
            if cursor <= INIT_PID {
                break;
            }
            let Some(ancestor) = self
                .slot_of
                .get(&Pid::new(cursor))
                .and_then(|&slot| entries[slot].as_ref())
            else {
                break;
            };
            if ancestor.is_ancestry_boundary() {
                break;
            }
            if let Some(app) = ancestor.app {
                match self
                    .registry
                    .identity(app)
                    .filter(|identity| identity.inheritable)
                {
                    Some(identity) => return Some((identity.clone(), false)),
                    None => break,
                }
            }
            if let Some(identity) = self.resolver.desktop_match(&ancestor.facts()) {
                return Some((identity, false));
            }
            cursor = ancestor.stat.parent;
        }
        Some((self.resolver.by_name(&facts), false))
    }
}
