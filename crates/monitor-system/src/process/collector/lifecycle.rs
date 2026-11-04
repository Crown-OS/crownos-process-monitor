use monitor_core::Pid;

use super::{NEW_PROCESS_SECONDS, ProcessCollector};
use crate::process::entry::{ProcessEntry, Refresh, TickFacts};
use crate::process::row::EndedProcess;

impl ProcessCollector {
    pub(super) fn track(&mut self, pid: Pid, tick: &TickFacts, now_unix: u64) -> Option<usize> {
        if let Some(&slot) = self.slot_of.get(&pid) {
            let outcome = match self.entries[slot].as_mut() {
                Some(entry) => {
                    let outcome = entry.refresh(&mut self.buffer, tick);
                    match outcome {
                        Refresh::Executed => entry.read_identity(&self.root, &mut self.buffer),
                        Refresh::Alive
                            if now_unix.saturating_sub(entry.started_unix)
                                < NEW_PROCESS_SECONDS =>
                        {
                            if entry.reread_cgroup(&self.root, &mut self.buffer) {
                                entry.app = None;
                            }
                        }
                        Refresh::Alive | Refresh::Gone => {}
                    }
                    outcome
                }
                None => Refresh::Gone,
            };
            if !matches!(outcome, Refresh::Gone) {
                return Some(slot);
            }
            self.end(slot, now_unix);
        }
        let entry = ProcessEntry::open(&self.root, pid, &mut self.buffer, tick, now_unix)?;
        let slot = match self.free_slots.pop() {
            Some(slot) => {
                self.entries[slot] = Some(entry);
                slot
            }
            None => {
                self.entries.push(Some(entry));
                self.entries.len() - 1
            }
        };
        self.slot_of.insert(pid, slot);
        Some(slot)
    }

    fn end(&mut self, slot: usize, now_unix: u64) {
        let Some(entry) = self.entries[slot].take() else {
            return;
        };
        if self.slot_of.get(&entry.key.pid) == Some(&slot) {
            self.slot_of.remove(&entry.key.pid);
        }
        self.free_slots.push(slot);
        if entry.stat.is_kernel_thread() {
            return;
        }
        let identity = entry.app.and_then(|app| self.registry.identity(app));
        let (app, app_name) = match identity {
            Some(identity) => (identity.key.clone(), identity.name.clone()),
            None => {
                let fallback = self.resolver.by_name(&entry.facts());
                (fallback.key, fallback.name)
            }
        };
        self.ended.push(EndedProcess {
            key: entry.key,
            peak_memory: entry.peak_memory(),
            name: entry.comm,
            app,
            app_name,
            started_unix: entry.started_unix,
            ended_unix: now_unix,
        });
    }

    pub(super) fn sweep(&mut self, generation: u64, now_unix: u64) {
        for slot in 0..self.entries.len() {
            if self.entries[slot]
                .as_ref()
                .is_some_and(|entry| entry.seen != generation)
            {
                self.end(slot, now_unix);
            }
        }
        self.ended.prune(now_unix);
    }
}
