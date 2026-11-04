use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use monitor_core::{Interest, Page, WindowState};
use monitor_engine::history::HistoryReader;
use monitor_engine::{Domain, Engine, EngineConfig, EngineHandle, Notifications, Snapshot};
use tokio::sync::watch;

const PATIENCE: Duration = Duration::from_secs(10);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("monitor-engine-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        Self(path)
    }

    fn database(&self) -> PathBuf {
        self.0.join("history.sqlite")
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn start(directory: &TempDirectory, interest: Interest) -> EngineHandle {
    let config = EngineConfig {
        interest,
        history_path: Some(directory.database()),
        notifications: Notifications::Off,
        ..EngineConfig::default()
    };
    match Engine::start(config) {
        Ok(engine) => engine,
        Err(error) => panic!("engine did not start: {error}"),
    }
}

fn block_on<T>(future: impl Future<Output = T>) -> T {
    match tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime.block_on(future),
        Err(error) => panic!("no runtime: {error}"),
    }
}

async fn next(snapshots: &mut watch::Receiver<Arc<Snapshot>>, within: Duration) -> Arc<Snapshot> {
    match tokio::time::timeout(within, snapshots.changed()).await {
        Ok(Ok(())) => snapshots.borrow_and_update().clone(),
        Ok(Err(_)) => panic!("the engine stopped publishing"),
        Err(_) => panic!("no snapshot within {within:?}"),
    }
}

#[test]
fn publishes_snapshots_records_a_session_and_shuts_down() {
    let directory = TempDirectory::new("publish");
    let engine = start(&directory, Interest::default());
    let mut snapshots = engine.snapshots();
    let (first, second) = block_on(async {
        let first = next(&mut snapshots, PATIENCE).await;
        (first, next(&mut snapshots, PATIENCE).await)
    });
    assert!(second.sequence > first.sequence);
    assert!(second.elapsed.seconds > 0.0);
    assert!(!second.processes.processes.is_empty());
    assert!(
        second
            .processes
            .apps
            .iter()
            .any(|app| app.process_count > 0)
    );
    assert!(second.memory.total.get() > 0);
    assert!(Arc::ptr_eq(&first.cpu_info, &second.cpu_info));

    let stopping = Instant::now();
    assert!(engine.shutdown().is_ok());
    assert!(
        stopping.elapsed() < Duration::from_secs(5),
        "shutdown joins promptly"
    );

    let reader = HistoryReader::open(&directory.database());
    assert!(
        reader
            .and_then(|reader| reader.recording_since())
            .is_ok_and(|since| since.is_some()),
        "the run left a session row"
    );
}

#[test]
fn inspects_on_the_sampler_and_reports_drive_actions() {
    let directory = TempDirectory::new("inspect");
    let engine = start(&directory, Interest::default());
    let control = engine.control();
    let mut snapshots = engine.snapshots();
    let mut drives = control.drive_outcomes();
    block_on(async {
        let snapshot = next(&mut snapshots, PATIENCE).await;
        let own = std::process::id();
        let Some(row) = snapshot
            .processes
            .processes
            .iter()
            .find(|row| row.key.pid.get() == own)
        else {
            panic!("own process missing");
        };
        let details = control.inspect(row.key).await;
        assert!(details.is_ok_and(|details| details.identity.key == row.key));
        let unmount = monitor_engine::Command::Drive {
            block_device: "no-such-device".into(),
            action: monitor_engine::DriveAction::Unmount,
        };
        assert!(control.send(unmount).is_ok());
        let outcome = tokio::time::timeout(PATIENCE, drives.recv()).await;
        assert!(outcome.is_ok_and(|outcome| outcome.is_ok_and(|outcome| outcome.result.is_err())));
    });
    assert!(engine.shutdown().is_ok());
    let stopped = block_on(control.inspect(monitor_core::ProcessKey {
        pid: monitor_core::Pid::new(1),
        start_ticks: 0,
    }));
    assert!(stopped.is_err());
}

#[test]
fn an_interest_change_samples_at_once() {
    let directory = TempDirectory::new("interest");
    let minimized = Interest {
        window: WindowState::Minimized,
        ..Interest::default()
    };
    let engine = start(&directory, minimized);
    let mut snapshots = engine.snapshots();
    block_on(async {
        let first = next(&mut snapshots, PATIENCE).await;
        assert_eq!(first.interest.window, WindowState::Minimized);
        let asked = Instant::now();
        let cpu_page = Interest {
            page: Page::Cpu,
            ..Interest::default()
        };
        assert!(engine.set_interest(cpu_page).is_ok());
        let switched = next(&mut snapshots, PATIENCE).await;
        assert_eq!(switched.interest.page, Page::Cpu);
        assert!(
            asked.elapsed() < Duration::from_secs(2),
            "sampled {:?} after the page switch, not on the 5 s cadence",
            asked.elapsed()
        );
    });
}

fn own_cpu_seconds() -> f64 {
    const UTIME: usize = 11;
    const STIME: usize = 12;
    let ticks = rustix::param::clock_ticks_per_second().max(1) as f64;
    let Ok(stat) = std::fs::read_to_string("/proc/self/stat") else {
        return 0.0;
    };
    let fields: Vec<&str> = stat
        .rsplit_once(')')
        .map_or("", |(_, rest)| rest)
        .split_whitespace()
        .collect();
    let field = |index: usize| {
        fields
            .get(index)
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    (field(UTIME) + field(STIME)) / ticks
}

const COST_LABELS: [&str; 9] = [
    "processes",
    "fd index",
    "cpu",
    "memory",
    "disk",
    "network",
    "gpu",
    "sensors",
    "power",
];

fn domain_costs(snapshot: &Snapshot) -> [Duration; 9] {
    let costs = &snapshot.costs;
    [
        costs.get(Domain::Processes),
        costs.fd_index,
        costs.get(Domain::Cpu),
        costs.get(Domain::Memory),
        costs.get(Domain::Disk),
        costs.get(Domain::Network),
        costs.get(Domain::Gpu),
        costs.get(Domain::Sensors),
        costs.get(Domain::Power),
    ]
}

struct Measurement {
    mean_costs_ms: Vec<(&'static str, f64)>,
    mean_tick_ms: f64,
    mean_clone_ms: f64,
    cpu_percent: f64,
    ticks: usize,
}

fn measure(engine: &EngineHandle, interest: Interest, run_for: Duration) -> Measurement {
    let mut snapshots = engine.snapshots();
    assert!(engine.set_interest(interest).is_ok());
    block_on(async {
        let _ = next(&mut snapshots, PATIENCE).await;
        let started = Instant::now();
        let cpu_before = own_cpu_seconds();
        let (mut ticks, mut tick_cost, mut clone_cost) = (0, 0.0, 0.0);
        let mut costs = vec![0.0; COST_LABELS.len()];
        while started.elapsed() < run_for {
            let snapshot = next(&mut snapshots, PATIENCE).await;
            let cloning = Instant::now();
            let copy = Snapshot::clone(&snapshot);
            clone_cost += cloning.elapsed().as_secs_f64();
            drop(copy);
            tick_cost += snapshot.tick_cost.as_secs_f64();
            for (sum, cost) in costs.iter_mut().zip(domain_costs(&snapshot)) {
                *sum += cost.as_secs_f64();
            }
            ticks += 1;
        }
        let wall = started.elapsed().as_secs_f64();
        let count = ticks.max(1) as f64;
        Measurement {
            mean_costs_ms: COST_LABELS
                .iter()
                .zip(costs)
                .map(|(label, sum)| (*label, sum / count * 1e3))
                .collect(),
            mean_tick_ms: tick_cost / count * 1e3,
            mean_clone_ms: clone_cost / count * 1e3,
            cpu_percent: (own_cpu_seconds() - cpu_before) / wall * 100.0,
            ticks,
        }
    })
}

/// Long enough for the first PSS round-robin over every process.
const WARM_UP: Duration = Duration::from_secs(30);

/// `cargo test --release -p monitor-engine --test engine -- --ignored --nocapture`
#[test]
#[ignore = "runs for three and a half minutes on real hardware"]
fn benchmark_tick_cost_and_own_cpu() {
    let directory = TempDirectory::new("benchmark");
    let engine = start(&directory, Interest::default());
    let minute = Duration::from_secs(60);
    let warm_up = measure(&engine, Interest::default(), WARM_UP);
    println!(
        "warm-up (first PSS pass over every process): {:.2} ms/tick, own CPU {:.3} %",
        warm_up.mean_tick_ms, warm_up.cpu_percent
    );
    let runs = [
        ("Overview, visible, 1 s", Interest::default(), minute),
        (
            "CPU, visible, 1 s",
            Interest {
                page: Page::Cpu,
                ..Interest::default()
            },
            minute,
        ),
        (
            "minimized, 5 s",
            Interest {
                window: WindowState::Minimized,
                ..Interest::default()
            },
            minute,
        ),
    ];
    for (label, interest, run_for) in runs {
        let result = measure(&engine, interest, run_for);
        println!(
            "{label}: {} ticks, {:.2} ms/tick, {:.3} ms/clone, own CPU {:.3} % of one core",
            result.ticks, result.mean_tick_ms, result.mean_clone_ms, result.cpu_percent
        );
        let costs: Vec<String> = result
            .mean_costs_ms
            .iter()
            .map(|(label, ms)| format!("{label} {ms:.2}"))
            .collect();
        println!("  ms per tick: {}", costs.join(", "));
    }
}
