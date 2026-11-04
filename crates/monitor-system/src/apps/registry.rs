use std::collections::HashMap;
use std::sync::Arc;

use monitor_core::{AppKey, Bytes, BytesPerSecond, Coverage, Percent};

use super::resolver::AppIdentity;
use super::role::{ProcessRole, RoleBreakdown};
use super::row::{AppRow, MemoryMethod};
use super::scope::{ScopeReading, ScopeUsage};
use crate::process::ExternalUsage;

/// One process's contribution to its app this tick.
pub(crate) struct MemberFigures<'a> {
    pub cpu: Percent,
    pub resident: Bytes,
    /// `None` when PSS is unknown or stale.
    pub proportional: Option<Bytes>,
    pub disk_read: BytesPerSecond,
    pub disk_write: BytesPerSecond,
    pub threads: u32,
    pub role: ProcessRole,
    /// The member's cgroup when that cgroup belongs to the app.
    pub owned_scope: Option<&'a Arc<str>>,
    pub partial: bool,
    pub started_unix: u64,
}

#[derive(Default)]
enum ScopeCandidate {
    #[default]
    None,
    Single(Arc<str>),
    Mixed,
}

#[derive(Default)]
struct Totals {
    members: u32,
    threads: u32,
    cpu: Percent,
    resident: Bytes,
    proportional: Bytes,
    all_proportional: bool,
    disk_read: BytesPerSecond,
    disk_write: BytesPerSecond,
    roles: RoleBreakdown,
    scope: ScopeCandidate,
    partial: bool,
    started_unix: u64,
}

struct AppState {
    identity: AppIdentity,
    totals: Totals,
    scope_usage: Option<ScopeUsage>,
    unreadable_scope: Option<Arc<str>>,
    row: usize,
}

/// Apps alive right now, in a slab with reused slots so a steady set of
/// apps allocates nothing per tick.
#[derive(Default)]
pub(crate) struct AppRegistry {
    slots: Vec<Option<AppState>>,
    free: Vec<usize>,
    by_key: HashMap<AppKey, usize>,
    buffer: Vec<u8>,
}

impl AppRegistry {
    pub fn intern(&mut self, identity: AppIdentity) -> usize {
        if let Some(&slot) = self.by_key.get(&identity.key) {
            return slot;
        }
        let state = AppState {
            identity,
            totals: Totals::default(),
            scope_usage: None,
            unreadable_scope: None,
            row: 0,
        };
        let key = state.identity.key.clone();
        let slot = match self.free.pop() {
            Some(slot) => {
                self.slots[slot] = Some(state);
                slot
            }
            None => {
                self.slots.push(Some(state));
                self.slots.len() - 1
            }
        };
        self.by_key.insert(key, slot);
        slot
    }

    pub fn slot_of(&self, key: &AppKey) -> Option<usize> {
        self.by_key.get(key).copied()
    }

    pub fn identity(&self, slot: usize) -> Option<&AppIdentity> {
        self.slots.get(slot)?.as_ref().map(|state| &state.identity)
    }

    /// Index of the app's row in the latest [`AppRegistry::finish`] output.
    pub fn row_of(&self, slot: usize) -> usize {
        self.slots
            .get(slot)
            .and_then(Option::as_ref)
            .map_or(0, |state| state.row)
    }

    pub fn begin(&mut self) {
        for state in self.slots.iter_mut().flatten() {
            state.totals = Totals {
                all_proportional: true,
                started_unix: u64::MAX,
                ..Totals::default()
            };
        }
    }

    pub fn add(&mut self, slot: usize, member: &MemberFigures<'_>) {
        let Some(Some(state)) = self.slots.get_mut(slot) else {
            return;
        };
        let totals = &mut state.totals;
        totals.members += 1;
        totals.threads += member.threads;
        totals.cpu += member.cpu;
        totals.resident += member.resident;
        match member.proportional {
            Some(proportional) => totals.proportional += proportional,
            None => totals.all_proportional = false,
        }
        totals.disk_read += member.disk_read;
        totals.disk_write += member.disk_write;
        totals.roles.add(member.role);
        totals.partial |= member.partial;
        totals.started_unix = totals.started_unix.min(member.started_unix);
        totals.scope = match (std::mem::take(&mut totals.scope), member.owned_scope) {
            (ScopeCandidate::None, Some(scope)) => ScopeCandidate::Single(scope.clone()),
            (ScopeCandidate::Single(current), Some(scope)) if current == *scope => {
                ScopeCandidate::Single(current)
            }
            _ => ScopeCandidate::Mixed,
        };
    }

    /// Builds one row per app with members and frees the slots of apps that
    /// have none left.
    pub fn finish(&mut self, seconds: Option<f64>, logical_cpus: u32, rows: &mut Vec<AppRow>) {
        rows.clear();
        for slot in 0..self.slots.len() {
            let Some(state) = &mut self.slots[slot] else {
                continue;
            };
            if state.totals.members == 0 {
                self.by_key.remove(&state.identity.key);
                self.slots[slot] = None;
                self.free.push(slot);
                continue;
            }
            state.row = rows.len();
            rows.push(state.build_row(&mut self.buffer, seconds, logical_cpus));
        }
    }
}

impl AppState {
    fn build_row(
        &mut self,
        buffer: &mut Vec<u8>,
        seconds: Option<f64>,
        logical_cpus: u32,
    ) -> AppRow {
        let scope = match &self.totals.scope {
            ScopeCandidate::Single(scope) => Some(scope.clone()),
            ScopeCandidate::None | ScopeCandidate::Mixed => None,
        };
        let cgroup = scope
            .as_ref()
            .and_then(|scope| self.read_scope(scope, buffer, seconds));
        let totals = &self.totals;
        let (memory, memory_method) = match cgroup {
            Some(reading) => (reading.memory, MemoryMethod::Cgroup),
            None if totals.all_proportional => (totals.proportional, MemoryMethod::Pss),
            None => (totals.resident, MemoryMethod::Rss),
        };
        let cpu = cgroup.and_then(|reading| reading.cpu).unwrap_or(totals.cpu);
        AppRow {
            key: self.identity.key.clone(),
            name: self.identity.name.clone(),
            icon: self.identity.icon.clone(),
            kind: self.identity.key.kind(),
            process_count: totals.members,
            threads: totals.threads,
            cpu,
            cpu_machine: Percent::new(cpu.get() / logical_cpus.max(1) as f32),
            memory,
            memory_method,
            resident: totals.resident,
            disk_read: totals.disk_read,
            disk_write: totals.disk_write,
            external: ExternalUsage::default(),
            roles: totals.roles,
            scope,
            frozen: cgroup.is_some_and(|reading| reading.frozen),
            started_unix: totals.started_unix,
            coverage: if totals.partial {
                Coverage::OwnProcesses
            } else {
                Coverage::Full
            },
        }
    }

    fn read_scope(
        &mut self,
        scope: &Arc<str>,
        buffer: &mut Vec<u8>,
        seconds: Option<f64>,
    ) -> Option<ScopeReading> {
        if self.unreadable_scope.as_ref() == Some(scope) {
            return None;
        }
        if self
            .scope_usage
            .as_ref()
            .is_none_or(|usage| usage.path != *scope)
        {
            self.scope_usage = ScopeUsage::open(scope);
            if self.scope_usage.is_none() {
                self.unreadable_scope = Some(scope.clone());
                return None;
            }
        }
        self.scope_usage.as_mut()?.read(buffer, seconds)
    }
}

#[cfg(test)]
mod tests {
    use monitor_core::AppKind;

    use super::*;

    fn identity(id: &str) -> AppIdentity {
        AppIdentity {
            key: AppKey::new(AppKind::Desktop, id),
            name: id.into(),
            icon: None,
            inheritable: true,
        }
    }

    fn member(cpu: f32, resident: u64, proportional: Option<u64>) -> MemberFigures<'static> {
        MemberFigures {
            cpu: Percent::new(cpu),
            resident: Bytes::new(resident),
            proportional: proportional.map(Bytes::new),
            disk_read: BytesPerSecond::ZERO,
            disk_write: BytesPerSecond::ZERO,
            threads: 2,
            role: ProcessRole::Main,
            owned_scope: None,
            partial: false,
            started_unix: 100,
        }
    }

    #[test]
    fn aggregates_and_picks_memory_method() {
        let mut registry = AppRegistry::default();
        let browser = registry.intern(identity("browser"));
        let editor = registry.intern(identity("editor"));
        assert_eq!(registry.intern(identity("browser")), browser);
        registry.begin();
        registry.add(browser, &member(10.0, 100, Some(60)));
        registry.add(browser, &member(5.0, 50, Some(30)));
        registry.add(editor, &member(1.0, 70, None));
        let mut rows = Vec::new();
        registry.finish(Some(1.0), 4, &mut rows);
        let browser_row = &rows[registry.row_of(browser)];
        assert_eq!(browser_row.process_count, 2);
        assert_eq!(browser_row.cpu.get(), 15.0);
        assert_eq!(browser_row.cpu_machine.get(), 3.75);
        assert_eq!(browser_row.memory_method, MemoryMethod::Pss);
        assert_eq!(browser_row.memory, Bytes::new(90));
        let editor_row = &rows[registry.row_of(editor)];
        assert_eq!(editor_row.memory_method, MemoryMethod::Rss);
        assert_eq!(editor_row.memory, Bytes::new(70));

        registry.begin();
        registry.add(browser, &member(1.0, 1, Some(1)));
        registry.finish(Some(1.0), 4, &mut rows);
        assert_eq!(rows.len(), 1);
        assert!(
            registry
                .slot_of(&AppKey::new(AppKind::Desktop, "editor"))
                .is_none()
        );
        assert_eq!(
            registry.intern(identity("viewer")),
            editor,
            "freed slot is reused"
        );
    }
}
