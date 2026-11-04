use std::time::{Duration, Instant};

use monitor_core::{Page, SampleContext};

use super::{OVERVIEW_PSS_PER_TICK, PSS_PER_TICK, PSS_TIME_BUDGET, ProcessCollector, RollupScope};
use crate::process::row::ProcessState;

impl ProcessCollector {
    pub(super) fn refresh_details(&mut self, context: &SampleContext<'_>, seconds: Option<f64>) {
        let interest = context.interest;
        let now = context.tick.monotonic();
        let inspected = interest.inspected.and_then(|key| {
            self.slot_of.get(&key.pid).copied().filter(|&slot| {
                self.entries[slot]
                    .as_ref()
                    .is_some_and(|entry| entry.key == key)
            })
        });
        if let Some(entry) = inspected.and_then(|slot| self.entries[slot].as_mut()) {
            entry.read_rollup(&self.root, &mut self.buffer, self.pss_cycle, now);
            entry.read_status(&self.root, &mut self.buffer);
        }
        if interest.shows(Page::Memory) {
            self.refresh_rollups_round_robin(PSS_PER_TICK, RollupScope::Every, now);
        } else if interest.shows(Page::Overview) {
            self.refresh_rollups_round_robin(
                OVERVIEW_PSS_PER_TICK,
                RollupScope::OutsideAppScopes,
                now,
            );
        }
        let expanded = interest
            .expanded_app
            .as_ref()
            .filter(|_| interest.is_visible())
            .and_then(|key| self.registry.slot_of(key));
        let io_shown = interest.shows(Page::Disk);
        for &slot in &self.row_slots {
            let Some(entry) = self.entries[slot].as_mut() else {
                continue;
            };
            if expanded.is_some() && entry.app == expanded {
                entry.read_status(&self.root, &mut self.buffer);
            }
            if io_shown || inspected == Some(slot) {
                if !entry.stat.is_kernel_thread() {
                    entry.read_io(&self.root, &mut self.buffer, seconds);
                }
            } else {
                entry.pause_io();
            }
        }
    }

    fn refresh_rollups_round_robin(&mut self, per_tick: usize, scope: RollupScope, now: Duration) {
        let count = self.row_slots.len();
        let deadline = Instant::now() + PSS_TIME_BUDGET;
        let mut refreshed = 0;
        for _ in 0..count {
            if refreshed == per_tick || Instant::now() >= deadline {
                break;
            }
            if self.pss_cursor >= count {
                self.pss_cursor = 0;
                self.pss_cycle += 1;
            }
            let slot = self.row_slots[self.pss_cursor];
            self.pss_cursor += 1;
            let Some(entry) = self.entries[slot].as_mut() else {
                continue;
            };
            if entry.rollup_denied
                || entry.stat.is_kernel_thread()
                || ProcessState::from_code(entry.stat.state) == ProcessState::Zombie
                || (scope == RollupScope::OutsideAppScopes && entry.scope_path.is_some())
                || entry.revalidate_rollup(self.pss_cycle, now)
            {
                continue;
            }
            entry.read_rollup(&self.root, &mut self.buffer, self.pss_cycle, now);
            refreshed += 1;
        }
    }
}
