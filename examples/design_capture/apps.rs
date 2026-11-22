//! A believable laptop session: desktop apps, a build in a terminal, services and kernel threads.

use std::sync::Arc;
use std::time::Duration;

use monitor_core::{
    AppKey, AppKind, Bytes, BytesPerSecond, Coverage, Percent, Pid, ProcessKey, Uid,
};
use monitor_engine::system::{
    AppRow, EndedProcess, ExternalUsage, MemoryMethod, ProcessRole, ProcessRow, ProcessState,
    ProcessTable, RoleBreakdown,
};

use crate::wave::Wave;

pub const LOGICAL_CPUS: u32 = 16;
pub const USER_UID: u32 = 1000;
pub const USER_NAME: &str = "alex";
/// Fake start ticks never match a live process, so nothing here can reach a real one.
pub const START_TICKS: u64 = 7;
const MIB: u64 = 1 << 20;
const SESSION_START_SECONDS: u64 = 3 * 3600;

pub struct ProcessSpec {
    pub pid: u32,
    pub name: &'static str,
    pub role: ProcessRole,
    pub cpu_core_percent: f32,
    pub memory_mib: u64,
    pub threads: u32,
    pub command_line: &'static str,
    pub executable: Option<&'static str>,
}

pub struct AppSpec {
    pub kind: AppKind,
    pub id: &'static str,
    pub name: &'static str,
    pub scoped: bool,
    pub root: bool,
    pub new: bool,
    pub download_kib: f64,
    pub processes: &'static [ProcessSpec],
}

const fn process(
    pid: u32,
    name: &'static str,
    role: ProcessRole,
    (cpu_core_percent, memory_mib, threads): (f32, u64, u32),
    command_line: &'static str,
    executable: Option<&'static str>,
) -> ProcessSpec {
    ProcessSpec {
        pid,
        name,
        role,
        cpu_core_percent,
        memory_mib,
        threads,
        command_line,
        executable,
    }
}

const FIREFOX_BIN: Option<&str> = Some("/usr/lib/firefox/firefox");
const CODE_BIN: Option<&str> = Some("/opt/visual-studio-code/code");
const CROWNTERM_BIN: Option<&str> = Some("/usr/bin/crownterm");

pub const APPS: &[AppSpec] = &[
    AppSpec {
        kind: AppKind::Desktop,
        id: "firefox",
        name: "Firefox",
        scoped: true,
        root: false,
        new: false,
        download_kib: 2400.0,
        processes: &[
            process(
                2311,
                "firefox",
                ProcessRole::Main,
                (6.2, 612, 94),
                "/usr/lib/firefox/firefox",
                FIREFOX_BIN,
            ),
            process(
                2398,
                "Web Content",
                ProcessRole::Tab,
                (14.5, 418, 28),
                "/usr/lib/firefox/firefox -contentproc -isForBrowser",
                FIREFOX_BIN,
            ),
            process(
                2402,
                "Web Content",
                ProcessRole::Tab,
                (3.1, 276, 24),
                "/usr/lib/firefox/firefox -contentproc -isForBrowser",
                FIREFOX_BIN,
            ),
            process(
                2417,
                "WebExtensions",
                ProcessRole::Extension,
                (0.8, 188, 22),
                "/usr/lib/firefox/firefox -contentproc -extension",
                FIREFOX_BIN,
            ),
            process(
                2366,
                "GPU Process",
                ProcessRole::Graphics,
                (4.4, 231, 18),
                "/usr/lib/firefox/firefox -contentproc -gpu",
                FIREFOX_BIN,
            ),
            process(
                2371,
                "RDD Process",
                ProcessRole::Utility,
                (0.3, 42, 6),
                "/usr/lib/firefox/firefox -contentproc -rdd",
                FIREFOX_BIN,
            ),
        ],
    },
    AppSpec {
        kind: AppKind::Terminal,
        id: "crownterm",
        name: "Terminal",
        scoped: true,
        root: false,
        new: false,
        download_kib: 0.0,
        processes: &[
            process(
                3104,
                "crownterm",
                ProcessRole::Main,
                (1.1, 96, 12),
                "crownterm",
                CROWNTERM_BIN,
            ),
            process(
                3117,
                "fish",
                ProcessRole::Other,
                (0.0, 14, 1),
                "-fish",
                Some("/usr/bin/fish"),
            ),
            process(
                3290,
                "cargo",
                ProcessRole::Other,
                (2.4, 128, 18),
                "cargo build --release",
                Some("/usr/bin/cargo"),
            ),
            process(
                3355,
                "rustc",
                ProcessRole::Other,
                (96.0, 1480, 17),
                "rustc --crate-name crownui --edition=2024",
                Some("/usr/bin/rustc"),
            ),
            process(
                3356,
                "rustc",
                ProcessRole::Other,
                (88.5, 912, 9),
                "rustc --crate-name impeller --edition=2024",
                Some("/usr/bin/rustc"),
            ),
        ],
    },
    AppSpec {
        kind: AppKind::Desktop,
        id: "code",
        name: "Code",
        scoped: true,
        root: false,
        new: false,
        download_kib: 12.0,
        processes: &[
            process(
                2804,
                "code",
                ProcessRole::Main,
                (2.6, 386, 41),
                "/opt/visual-studio-code/code",
                CODE_BIN,
            ),
            process(
                2851,
                "code",
                ProcessRole::Graphics,
                (1.9, 164, 12),
                "/opt/visual-studio-code/code --type=gpu-process",
                CODE_BIN,
            ),
            process(
                2873,
                "code",
                ProcessRole::Tab,
                (3.4, 522, 23),
                "/opt/visual-studio-code/code --type=renderer",
                CODE_BIN,
            ),
            process(
                2902,
                "code",
                ProcessRole::Utility,
                (1.2, 288, 15),
                "/opt/visual-studio-code/code --type=utility",
                CODE_BIN,
            ),
            process(
                2960,
                "rust-analyzer",
                ProcessRole::Other,
                (8.7, 1840, 24),
                "rust-analyzer",
                Some("/usr/bin/rust-analyzer"),
            ),
        ],
    },
    AppSpec {
        kind: AppKind::Flatpak,
        id: "com.spotify.Client",
        name: "Spotify",
        scoped: true,
        root: false,
        new: false,
        download_kib: 320.0,
        processes: &[
            process(
                2550,
                "spotify",
                ProcessRole::Main,
                (2.1, 342, 46),
                "/app/extra/share/spotify/spotify",
                Some("/app/extra/share/spotify/spotify"),
            ),
            process(
                2581,
                "spotify",
                ProcessRole::Graphics,
                (0.9, 118, 9),
                "/app/extra/share/spotify/spotify --type=gpu-process",
                Some("/app/extra/share/spotify/spotify"),
            ),
        ],
    },
    AppSpec {
        kind: AppKind::Desktop,
        id: "crownos-files",
        name: "Files",
        scoped: true,
        root: false,
        new: true,
        download_kib: 0.0,
        processes: &[process(
            3620,
            "crownos-files",
            ProcessRole::Main,
            (0.6, 84, 8),
            "crownos-files",
            Some("/usr/bin/crownos-files"),
        )],
    },
    AppSpec {
        kind: AppKind::Desktop,
        id: "crownos-process-monitor",
        name: "Process Monitor",
        scoped: true,
        root: false,
        new: false,
        download_kib: 0.0,
        processes: &[process(
            3512,
            "crownos-process-monitor",
            ProcessRole::Main,
            (1.4, 72, 9),
            "crownos-process-monitor",
            Some("/usr/bin/crownos-process-monitor"),
        )],
    },
    AppSpec {
        kind: AppKind::Container,
        id: "postgres-dev",
        name: "postgres-dev",
        scoped: false,
        root: false,
        new: false,
        download_kib: 0.0,
        processes: &[
            process(
                4020,
                "postgres",
                ProcessRole::Main,
                (0.4, 156, 1),
                "postgres -D /var/lib/postgresql/data",
                Some("/usr/lib/postgresql/16/bin/postgres"),
            ),
            process(
                4031,
                "postgres",
                ProcessRole::Other,
                (0.1, 38, 1),
                "postgres: checkpointer",
                Some("/usr/lib/postgresql/16/bin/postgres"),
            ),
        ],
    },
    AppSpec {
        kind: AppKind::Service,
        id: "pipewire",
        name: "PipeWire",
        scoped: false,
        root: false,
        new: false,
        download_kib: 0.0,
        processes: &[
            process(
                1702,
                "pipewire",
                ProcessRole::Main,
                (0.9, 22, 3),
                "/usr/bin/pipewire",
                Some("/usr/bin/pipewire"),
            ),
            process(
                1704,
                "wireplumber",
                ProcessRole::Other,
                (0.2, 34, 5),
                "/usr/bin/wireplumber",
                Some("/usr/bin/wireplumber"),
            ),
            process(
                1705,
                "pipewire-pulse",
                ProcessRole::Other,
                (0.3, 18, 3),
                "/usr/bin/pipewire-pulse",
                Some("/usr/bin/pipewire-pulse"),
            ),
        ],
    },
    AppSpec {
        kind: AppKind::Service,
        id: "NetworkManager",
        name: "NetworkManager",
        scoped: false,
        root: true,
        new: false,
        download_kib: 0.0,
        processes: &[process(
            812,
            "NetworkManager",
            ProcessRole::Main,
            (0.1, 21, 4),
            "/usr/bin/NetworkManager --no-daemon",
            Some("/usr/bin/NetworkManager"),
        )],
    },
    AppSpec {
        kind: AppKind::Service,
        id: "systemd",
        name: "systemd",
        scoped: false,
        root: true,
        new: false,
        download_kib: 0.0,
        processes: &[
            process(
                1,
                "systemd",
                ProcessRole::Main,
                (0.1, 14, 1),
                "/sbin/init",
                Some("/usr/lib/systemd/systemd"),
            ),
            process(
                402,
                "systemd-journald",
                ProcessRole::Other,
                (0.2, 26, 1),
                "/usr/lib/systemd/systemd-journald",
                Some("/usr/lib/systemd/systemd-journald"),
            ),
            process(
                790,
                "systemd-logind",
                ProcessRole::Other,
                (0.0, 9, 1),
                "/usr/lib/systemd/systemd-logind",
                Some("/usr/lib/systemd/systemd-logind"),
            ),
        ],
    },
    AppSpec {
        kind: AppKind::Kernel,
        id: "kernel",
        name: "Kernel",
        scoped: false,
        root: true,
        new: false,
        download_kib: 0.0,
        processes: &[
            process(98, "kswapd0", ProcessRole::Other, (0.4, 0, 1), "", None),
            process(
                214,
                "kworker/u32:2",
                ProcessRole::Other,
                (0.8, 0, 1),
                "",
                None,
            ),
            process(31, "ksoftirqd/3", ProcessRole::Other, (0.2, 0, 1), "", None),
        ],
    },
];

fn key(pid: u32) -> ProcessKey {
    ProcessKey {
        pid: Pid::new(pid),
        start_ticks: START_TICKS,
    }
}

fn process_row(spec: &ProcessSpec, app: usize, owner: &AppSpec, cpu: f32, now: u64) -> ProcessRow {
    let memory = Bytes::new(spec.memory_mib * MIB / 1000 * (985 + (cpu * 7.0) as u64 % 30));
    let download = owner.download_kib
        * 1024.0
        * f64::from(u8::from(
            spec.role == ProcessRole::Tab || spec.role == ProcessRole::Main,
        ));
    ProcessRow {
        key: key(spec.pid),
        parent: Pid::new(owner.processes.first().map_or(1, |main| main.pid)),
        uid: Uid::new(if owner.root { 0 } else { USER_UID }),
        name: spec.name.into(),
        command_line: spec.command_line.into(),
        executable: spec.executable.map(Arc::from),
        app,
        role: spec.role,
        state: if cpu > 50.0 {
            ProcessState::Running
        } else {
            ProcessState::Sleeping
        },
        threads: spec.threads,
        nice: 0,
        cpu: Percent::new(cpu),
        cpu_machine: Percent::new(cpu / LOGICAL_CPUS as f32),
        cpu_time: Duration::from_secs(u64::from(spec.pid) % 900 + 40),
        resident: memory,
        proportional: Some(memory),
        unique: Some(Bytes::new(spec.memory_mib * MIB * 3 / 4)),
        swap: Some(Bytes::new(spec.memory_mib * MIB / 16)),
        peak_resident: Some(Bytes::new(spec.memory_mib * MIB * 5 / 4)),
        minor_faults: 0,
        major_faults: 0,
        disk_read: BytesPerSecond::new(f64::from(cpu) * 4096.0),
        disk_write: BytesPerSecond::new(f64::from(cpu) * 1024.0),
        read_since_start: Some(Bytes::new(spec.memory_mib * MIB * 2)),
        written_since_start: Some(Bytes::new(spec.memory_mib * MIB / 2)),
        external: ExternalUsage {
            gpu: Percent::new(if spec.role == ProcessRole::Graphics {
                cpu * 2.5
            } else {
                0.0
            }),
            gpu_memory: Bytes::new(if spec.role == ProcessRole::Graphics {
                180 * MIB
            } else {
                0
            }),
            network_receive: BytesPerSecond::new(download),
            network_transmit: BytesPerSecond::new(download / 9.0),
            network_received_total: Bytes::new((download * 3600.0) as u64),
            network_transmitted_total: Bytes::new((download * 400.0) as u64),
        },
        started_unix: now - SESSION_START_SECONDS + u64::from(spec.pid % 600),
        is_new: owner.new,
        kernel_thread: owner.kind == AppKind::Kernel,
        coverage: Coverage::Full,
    }
}

fn app_row(spec: &AppSpec, processes: &[ProcessRow], now: u64) -> AppRow {
    let mut roles = RoleBreakdown::default();
    let mut external = ExternalUsage::default();
    for process in processes {
        roles.add(process.role);
        external += process.external;
    }
    let sum_percent = |pick: fn(&ProcessRow) -> Percent| {
        Percent::new(processes.iter().map(|process| pick(process).get()).sum())
    };
    let memory = Bytes::new(processes.iter().map(|process| process.memory().get()).sum());
    AppRow {
        key: AppKey::new(spec.kind, spec.id),
        name: spec.name.into(),
        icon: None,
        kind: spec.kind,
        process_count: processes.len() as u32,
        threads: processes.iter().map(|process| process.threads).sum(),
        cpu: sum_percent(|process| process.cpu),
        cpu_machine: sum_percent(|process| process.cpu_machine),
        memory,
        memory_method: MemoryMethod::default(),
        resident: memory,
        disk_read: BytesPerSecond::new(
            processes
                .iter()
                .map(|process| process.disk_read.get())
                .sum(),
        ),
        disk_write: BytesPerSecond::new(
            processes
                .iter()
                .map(|process| process.disk_write.get())
                .sum(),
        ),
        external,
        roles,
        scope: spec
            .scoped
            .then(|| Arc::from(format!("/design.slice/app-{}.scope", spec.id))),
        frozen: false,
        started_unix: now - SESSION_START_SECONDS,
        coverage: Coverage::Full,
    }
}

fn ended(now: u64) -> Vec<EndedProcess> {
    [
        ("make", 5120, 95, 48),
        ("rustc", 3301, 140, 1210),
        ("Web Content", 2440, 410, 260),
    ]
    .into_iter()
    .map(|(name, pid, ago, peak_mib)| EndedProcess {
        key: key(pid),
        name: name.into(),
        app: AppKey::new(AppKind::Terminal, "crownterm"),
        app_name: "Terminal".into(),
        started_unix: now - ago - 300,
        ended_unix: now - ago,
        peak_memory: Bytes::new(peak_mib * MIB),
    })
    .collect()
}

/// The process table at one tick; CPU wanders around each process's typical use.
pub fn table(wave: &mut Wave, now: u64) -> ProcessTable {
    let mut processes = Vec::new();
    let mut apps = Vec::with_capacity(APPS.len());
    for (index, spec) in APPS.iter().enumerate() {
        let first = processes.len();
        processes.extend(spec.processes.iter().map(|process| {
            let cpu = (process.cpu_core_percent * (0.8 + 0.4 * wave.noise())).max(0.0);
            process_row(process, index, spec, cpu, now)
        }));
        apps.push(app_row(spec, &processes[first..], now));
    }
    processes.sort_by_key(|process| process.key.pid);
    ProcessTable {
        gap: false,
        thread_count: processes.iter().map(|process| process.threads).sum(),
        processes,
        apps,
        ended: ended(now),
        logical_cpus: LOGICAL_CPUS,
        io_coverage: Coverage::Full,
        memory_coverage: Coverage::Full,
    }
}
