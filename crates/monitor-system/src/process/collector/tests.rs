use std::time::{Duration, Instant};

use monitor_core::{Bytes, BytesPerSecond, Coverage, Elapsed, FdIndex, Page, Percent, Tick};

use super::*;
use crate::apps::{
    AppSortKey, Direction, MemoryMethod, ProcessSortKey, sort_apps, sort_processes_of_app,
};
use crate::process::row::ExternalUsage;

const BENCHMARK_SAMPLES: u32 = 100;

struct Harness {
    collector: ProcessCollector,
    table: ProcessTable,
    previous: Tick,
    fds: FdIndex,
}

impl Harness {
    fn new() -> SourceResult<Self> {
        Ok(Self {
            collector: ProcessCollector::new()?,
            table: ProcessTable::default(),
            previous: Tick::now(),
            fds: FdIndex::default(),
        })
    }

    fn sample(&mut self, interest: &Interest, first: bool) -> SourceResult<()> {
        let tick = Tick::now();
        let elapsed = if first {
            Elapsed::default()
        } else {
            tick.since(&self.previous)
        };
        self.previous = tick;
        let context = SampleContext {
            tick,
            elapsed,
            interest,
            fds: &self.fds,
        };
        self.collector.sample(&context, &mut self.table)
    }
}

#[test]
fn collectors_move_to_the_sampler_thread() {
    fn assert_send<T: Send>() {}
    assert_send::<ProcessCollector>();
    assert_send::<crate::CpuCollector>();
    assert_send::<crate::MemoryCollector>();
    assert_send::<ProcessTable>();
}

fn own_pid() -> Pid {
    Pid::new(std::process::id())
}

#[test]
fn minimized_ticks_find_new_processes_on_the_history_pass() -> SourceResult<()> {
    let mut harness = Harness::new()?;
    let minimized = Interest {
        window: monitor_core::WindowState::Minimized,
        ..Interest::default()
    };
    harness.sample(&minimized, true)?;
    let Ok(mut child) = std::process::Command::new("sleep").arg("30").spawn() else {
        return Ok(());
    };
    let pid = Pid::new(child.id());
    harness.sample(&minimized, false)?;
    let between = harness.table.index_of(pid);
    assert!(harness.collector.listing_due(
        &minimized,
        Tick::now().monotonic() + MINIMIZED_LISTING_INTERVAL
    ));
    harness.sample(&Interest::default(), false)?;
    let visible = harness.table.index_of(pid);
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(between, None, "no /proc listing between minimized passes");
    assert!(visible.is_some(), "a visible tick lists /proc again");
    assert!(harness.table.index_of(own_pid()).is_some());
    Ok(())
}

#[test]
fn samples_live_processes() -> SourceResult<()> {
    let mut harness = Harness::new()?;
    let interest = Interest {
        page: Page::Memory,
        ..Interest::default()
    };
    harness.sample(&interest, true)?;
    assert!(harness.table.gap);
    let busy_until = Instant::now() + Duration::from_millis(150);
    while Instant::now() < busy_until {
        std::hint::black_box(0_u64);
    }
    harness.sample(&interest, false)?;
    let table = &harness.table;
    assert!(!table.gap);
    assert!(
        table
            .processes
            .windows(2)
            .all(|pair| pair[0].key.pid < pair[1].key.pid)
    );
    assert!(table.processes.iter().all(|row| row.app < table.apps.len()));
    let ceiling = 100.0 * table.logical_cpus as f32 + 1.0;
    assert!(
        table
            .processes
            .iter()
            .all(|row| (0.0..=ceiling).contains(&row.cpu.get()))
    );
    assert!(
        table
            .processes
            .iter()
            .all(|row| row.cpu_machine.get() <= 101.0)
    );

    let Some(own) = table
        .index_of(own_pid())
        .map(|index| &table.processes[index])
    else {
        panic!("own process missing from the table");
    };
    assert_eq!(own.uid, Uid::current());
    assert!(own.cpu.get() > 0.0, "the busy loop shows up");
    assert!(own.resident.get() > 0);
    assert!(own.threads >= 1);
    assert!(own.executable.is_some());
    assert_eq!(own.coverage, Coverage::Full);
    assert!(harness.collector.own_pids().contains(&own_pid()));

    let members: u32 = table.apps.iter().map(|app| app.process_count).sum();
    assert_eq!(members as usize, table.processes.len());
    let kernel_threads_visible =
        std::fs::read_to_string("/proc/2/comm").is_ok_and(|comm| comm.trim_end() == "kthreadd");
    if kernel_threads_visible {
        assert!(
            table
                .apps
                .iter()
                .any(|app| app.kind == monitor_core::AppKind::Kernel)
        );
    }
    let own_app = &table.apps[own.app];
    assert!(own_app.process_count >= 1);
    if own_app.memory_method == MemoryMethod::Cgroup {
        assert!(own_app.scope.is_some());
    }
    Ok(())
}

#[test]
fn attributes_external_usage() -> SourceResult<()> {
    let mut harness = Harness::new()?;
    harness.sample(&Interest::default(), true)?;
    let usage = ExternalUsage {
        gpu: Percent::new(12.0),
        gpu_memory: Bytes::new(1024),
        network_receive: BytesPerSecond::new(10.0),
        network_transmit: BytesPerSecond::new(5.0),
        network_received_total: Bytes::new(4096),
        network_transmitted_total: Bytes::new(512),
    };
    harness.table.apply_external([
        (own_pid(), usage),
        (own_pid(), usage),
        (Pid::new(u32::MAX), usage),
    ]);
    let Some(index) = harness.table.index_of(own_pid()) else {
        panic!("own process missing");
    };
    let row = &harness.table.processes[index];
    assert_eq!(row.external.gpu.get(), 24.0);
    assert_eq!(
        harness.table.apps[row.app].external.gpu_memory,
        Bytes::new(2048)
    );
    assert_eq!(row.external.network_received_total, Bytes::new(8192));
    harness.table.apply_external(std::iter::empty());
    let row = &harness.table.processes[index];
    assert_eq!(row.external, ExternalUsage::default());
    assert_eq!(
        harness.table.apps[row.app].external,
        ExternalUsage::default()
    );
    Ok(())
}

#[test]
fn sorts_apps_and_members() -> SourceResult<()> {
    let mut harness = Harness::new()?;
    harness.sample(&Interest::default(), true)?;
    let table = &harness.table;
    let mut order = Vec::new();
    sort_apps(
        &table.apps,
        AppSortKey::Memory,
        Direction::Descending,
        &mut order,
    );
    assert_eq!(order.len(), table.apps.len());
    assert!(
        order
            .windows(2)
            .all(|pair| table.apps[pair[0]].memory >= table.apps[pair[1]].memory)
    );
    sort_apps(
        &table.apps,
        AppSortKey::Name,
        Direction::Ascending,
        &mut order,
    );
    assert_eq!(order.len(), table.apps.len());
    let Some(own) = table.process(table.processes[table.index_of(own_pid()).unwrap_or(0)].key)
    else {
        panic!("own process missing");
    };
    sort_processes_of_app(
        table,
        own.app,
        ProcessSortKey::Pid,
        Direction::Ascending,
        &mut order,
    );
    assert!(
        order
            .iter()
            .any(|&index| table.processes[index].key.pid == own_pid())
    );
    assert!(
        order
            .iter()
            .all(|&index| table.processes[index].app == own.app)
    );
    Ok(())
}

#[test]
fn inspects_through_collector() -> SourceResult<()> {
    let mut harness = Harness::new()?;
    harness.sample(&Interest::default(), true)?;
    let Some(key) = harness
        .table
        .index_of(own_pid())
        .map(|index| harness.table.processes[index].key)
    else {
        panic!("own process missing");
    };
    let details = harness.collector.inspect(key)?;
    let row_app = &harness.table.apps
        [harness.table.processes[harness.table.index_of(own_pid()).unwrap_or(0)].app];
    assert_eq!(details.identity.app, row_app.key);
    Ok(())
}

#[test]
fn inspected_process_gets_pss_and_peak() -> SourceResult<()> {
    let mut harness = Harness::new()?;
    harness.sample(&Interest::default(), true)?;
    let Some(key) = harness
        .table
        .index_of(own_pid())
        .map(|index| harness.table.processes[index].key)
    else {
        panic!("own process missing");
    };
    let interest = Interest {
        page: Page::Disk,
        inspected: Some(key),
        ..Interest::default()
    };
    harness.sample(&interest, false)?;
    let Some(row) = harness.table.process(key) else {
        panic!("own process missing");
    };
    assert!(row.proportional.is_some_and(|pss| pss.get() > 0));
    assert!(row.unique.is_some());
    assert!(row.read_since_start.is_some() && row.written_since_start.is_some());
    assert!(
        row.peak_resident
            .is_some_and(|peak| peak >= row.resident || peak.get() > 0)
    );
    Ok(())
}

#[test]
#[ignore = "micro benchmark; run with --ignored --nocapture"]
fn benchmark_hundred_samples() -> SourceResult<()> {
    for page in [Page::Cpu, Page::Overview, Page::Memory] {
        let mut harness = Harness::new()?;
        let interest = Interest {
            page,
            ..Interest::default()
        };
        harness.sample(&interest, true)?;
        let started = Instant::now();
        for _ in 0..BENCHMARK_SAMPLES {
            harness.sample(&interest, false)?;
        }
        let mean = started.elapsed().as_secs_f64() * 1_000_000.0 / f64::from(BENCHMARK_SAMPLES);
        println!(
            "ProcessCollector ({page:?} page): {mean:.0} µs per sample over {BENCHMARK_SAMPLES} samples, {} processes, {} apps",
            harness.table.processes.len(),
            harness.table.apps.len()
        );
    }
    Ok(())
}
