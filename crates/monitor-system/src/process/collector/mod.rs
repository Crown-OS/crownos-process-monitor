use std::collections::HashMap;
use std::time::Duration;

use monitor_core::util::cadence::interval_elapsed;
use monitor_core::util::procfs::ProcRoot;
use monitor_core::{Collector, Interest, Pid, ProcessKey, SampleContext, SourceResult, Uid};

use super::ended::EndedRing;
use super::entry::{IdleReads, ProcessEntry, TickFacts};
use super::privileged::PrivilegedUsage;
use super::table::ProcessTable;
use crate::apps::AppRegistry;
use crate::apps::desktop::DesktopIndex;
use crate::apps::resolver::AppResolver;
use crate::inspect::{InspectContext, ProcessDetails};
use crate::util::{clock, cpulist, rlimit};

mod details;
mod grouping;
mod lifecycle;
mod rows;

/// smaps_rollup makes the kernel walk page tables, so PSS is refreshed for
/// at most this many processes per tick, round-robin, on the Memory page…
const PSS_PER_TICK: usize = 50;
/// …fewer on the Overview, whose app memory ring tolerates older PSS and
/// needs none for apps measured by their own cgroup…
const OVERVIEW_PSS_PER_TICK: usize = 12;
/// …and for no longer than this; one large browser process alone can take
/// a millisecond.
const PSS_TIME_BUDGET: Duration = Duration::from_millis(2);
/// Processes younger than this are flagged new and have their cgroup re-read.
const NEW_PROCESS_SECONDS: u64 = 10;
const MAX_ANCESTRY_DEPTH: usize = 32;
const INIT_PID: u32 = 1;
const CPU_COUNT_REFRESH_TICKS: u64 = 10;
const INITIAL_PROCESS_CAPACITY: usize = 2048;
/// Minimized, `/proc` is listed for new processes this often (the 15 s
/// history pass, less a second of tick jitter); in between the known
/// processes are re-read through their open `stat` descriptors.
const MINIMIZED_LISTING_INTERVAL: Duration = Duration::from_secs(14);

/// Which processes the PSS round-robin visits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RollupScope {
    /// The Memory page shows PSS per process.
    Every,
    /// Apps living in their own cgroup take memory from `memory.current`.
    OutsideAppScopes,
}

/// The process table hot path of plan §4.4: one persistent `stat`
/// descriptor per process, slab storage with reused slots, identity read
/// once per process lifetime.
pub struct ProcessCollector {
    root: ProcRoot,
    pids: Vec<Pid>,
    entries: Vec<Option<ProcessEntry>>,
    free_slots: Vec<usize>,
    slot_of: HashMap<Pid, usize>,
    row_slots: Vec<usize>,
    own_pids: Vec<Pid>,
    generation: u64,
    buffer: Vec<u8>,
    resolver: AppResolver,
    registry: AppRegistry,
    ended: EndedRing,
    own_uid: Uid,
    ticks_per_second: u64,
    page_size: u64,
    logical_cpus: u32,
    pss_cursor: usize,
    /// Completed round-robin passes; PSS read in this or the previous pass
    /// counts as fresh.
    pss_cycle: u64,
    listed_at: Option<Duration>,
}

impl ProcessCollector {
    /// Opens `/proc`, raises the open-file limit and indexes desktop entries.
    pub fn new() -> SourceResult<Self> {
        rlimit::raise_open_file_limit();
        Ok(Self {
            root: ProcRoot::open()?,
            pids: Vec::with_capacity(INITIAL_PROCESS_CAPACITY),
            entries: Vec::with_capacity(INITIAL_PROCESS_CAPACITY),
            free_slots: Vec::new(),
            slot_of: HashMap::with_capacity(INITIAL_PROCESS_CAPACITY),
            row_slots: Vec::with_capacity(INITIAL_PROCESS_CAPACITY),
            own_pids: Vec::with_capacity(INITIAL_PROCESS_CAPACITY),
            generation: 0,
            buffer: Vec::with_capacity(4096),
            resolver: AppResolver::new(DesktopIndex::load(), Uid::current()),
            registry: AppRegistry::default(),
            ended: EndedRing::default(),
            own_uid: Uid::current(),
            ticks_per_second: clock::clock_ticks_per_second(),
            page_size: clock::page_size(),
            logical_cpus: cpulist::online_cpu_count(),
            pss_cursor: 0,
            pss_cycle: 1,
            listed_at: None,
        })
    }

    fn listing_due(&self, interest: &Interest, now: Duration) -> bool {
        interest.is_visible() || interval_elapsed(self.listed_at, now, MINIMIZED_LISTING_INTERVAL)
    }

    /// The current user's processes as of the last sample, for the engine's
    /// `FdIndex` refresh.
    pub fn own_pids(&self) -> &[Pid] {
        &self.own_pids
    }

    /// Fills figures this user cannot read (other users' I/O and PSS) from
    /// the privileged helper; `observed_at` is the monotonic time of the
    /// reading. Rates come from consecutive readings and lapse after a few
    /// ticks without one.
    pub fn apply_privileged(&mut self, usage: &[PrivilegedUsage], observed_at: Duration) {
        for usage in usage {
            let Some(entry) = self
                .slot_of
                .get(&usage.key.pid)
                .and_then(|&slot| self.entries[slot].as_mut())
                .filter(|entry| entry.key == usage.key)
            else {
                continue;
            };
            entry.apply_privileged(usage, observed_at, self.generation, self.pss_cycle);
        }
    }

    /// Inspector details, resolved with this collector's app grouping and
    /// desktop index.
    pub fn inspect(&self, key: ProcessKey) -> SourceResult<ProcessDetails> {
        let entry = self
            .slot_of
            .get(&key.pid)
            .and_then(|&slot| self.entries[slot].as_ref())
            .filter(|entry| entry.key == key);
        let app = entry
            .and_then(|entry| entry.app)
            .and_then(|app| self.registry.identity(app))
            .map(|identity| (identity.key.clone(), identity.name.clone()));
        crate::inspect::inspect_with(
            key,
            &InspectContext {
                app,
                resolver: &self.resolver,
            },
        )
    }
}

impl Collector for ProcessCollector {
    type Output = ProcessTable;

    fn sample(
        &mut self,
        context: &SampleContext<'_>,
        output: &mut ProcessTable,
    ) -> SourceResult<()> {
        self.generation += 1;
        let generation = self.generation;
        let seconds = Some(context.elapsed.seconds)
            .filter(|&seconds| seconds > 0.0 && !context.elapsed.suspended);
        let now_unix = context.tick.wall_unix_seconds();
        if generation.is_multiple_of(CPU_COUNT_REFRESH_TICKS) {
            self.logical_cpus = cpulist::online_cpu_count();
        }
        self.resolver.refresh_desktop(context.tick.monotonic());
        let tick = TickFacts {
            generation,
            seconds,
            now: context.tick.monotonic(),
            idle_reads: if context.interest.is_visible() {
                IdleReads::EveryTick
            } else {
                IdleReads::Spaced
            },
            ticks_per_second: self.ticks_per_second,
            page_size: self.page_size,
        };
        if self.listing_due(context.interest, tick.now) {
            self.root.list_pids(&mut self.pids)?;
            self.listed_at = Some(tick.now);
        }
        self.row_slots.clear();
        for index in 0..self.pids.len() {
            let pid = self.pids[index];
            if let Some(slot) = self.track(pid, &tick, now_unix) {
                self.row_slots.push(slot);
            }
        }
        self.sweep(generation, now_unix);
        self.resolve_apps();
        self.refresh_details(context, seconds);
        self.build(seconds, now_unix, output);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
