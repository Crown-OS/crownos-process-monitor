//! A deterministic synthetic half hour for the headless UI tests; the app itself is fed by the
//! engine bridge.

use crownui::kit::icons;

use super::live::{AppEntry, AppList, HISTORY_CAPACITY, LiveSnapshot, ProcessEntry, push_sample};
use crate::components::app_table::RowMark;

const GIB: f32 = 1024.0 * 1024.0 * 1024.0;
const MIB: f32 = 1024.0 * 1024.0;
const MEMORY_TOTAL: u64 = 15 * 1024 * 1024 * 1024;
const VRAM_TOTAL: u64 = 8 * 1024 * 1024 * 1024;
const DISK_TOTAL: u64 = 1024 * 1024 * 1024 * 1024;
const SUSPEND_GAP: std::ops::Range<usize> = 1210..1240;
const DAYS_SHOWN: usize = 7;

#[derive(Debug, Clone, Copy)]
struct DemoGenerator {
    tick: u64,
    state: u64,
}

impl Default for DemoGenerator {
    fn default() -> Self {
        Self {
            tick: 0,
            state: 0x9e37_79b9_7f4a_7c15,
        }
    }
}

impl DemoGenerator {
    fn noise(&mut self) -> f32 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        (self.state >> 40) as f32 / (1_u64 << 24) as f32
    }

    fn wave(&mut self, base: f32, amplitude: f32, period: f32) -> f32 {
        let phase = self.tick as f32 / period;
        let swing = phase.sin() * 0.6 + (phase * 2.7).sin() * 0.25;
        (base + amplitude * swing + amplitude * 0.3 * (self.noise() - 0.5)).max(0.0)
    }

    fn seed(&mut self, snapshot: &mut LiveSnapshot) {
        for history in histories(snapshot) {
            history.clear();
            history.resize(HISTORY_CAPACITY, f32::NAN);
        }
        snapshot.memory.total_bytes = MEMORY_TOTAL;
        snapshot.gpu.vram_total_bytes = VRAM_TOTAL;
        snapshot.disk.total_bytes = DISK_TOTAL;
        snapshot.network.daily_bytes = (0..DAYS_SHOWN)
            .map(|day| (1.5 + (day as f32 * 1.3).sin().abs() * 3.0) * GIB)
            .collect();
        snapshot.apps = demo_apps();
        for sample in 0..HISTORY_CAPACITY {
            self.advance(snapshot);
            if SUSPEND_GAP.contains(&sample) {
                for history in histories(snapshot) {
                    if let Some(last) = history.last_mut() {
                        *last = f32::NAN;
                    }
                }
            }
        }
    }

    fn advance(&mut self, snapshot: &mut LiveSnapshot) {
        self.tick += 1;
        let user = self.wave(14.0, 10.0, 40.0);
        let system = self.wave(5.0, 3.0, 25.0);
        let iowait = self.wave(1.0, 1.5, 15.0);
        let cpu = &mut snapshot.cpu;
        push_sample(&mut cpu.user, user);
        push_sample(&mut cpu.system, system);
        push_sample(&mut cpu.iowait, iowait);
        push_sample(&mut cpu.total, (user + system + iowait).min(100.0));
        cpu.temperature_celsius = self.wave(58.0, 8.0, 60.0);
        cpu.frequency_mhz = self.wave(3400.0, 900.0, 20.0);
        cpu.load_average = [user / 10.0, user / 12.0, user / 14.0];
        push_sample(&mut cpu.load, cpu.load_average[0]);
        push_sample(&mut cpu.pressure, iowait * 2.0);
        push_sample(&mut cpu.context_switches_per_second, user * 400.0);
        push_sample(&mut cpu.running_tasks, (user / 8.0).round());

        let used_percent = self.wave(58.0, 6.0, 120.0).min(100.0);
        let memory = &mut snapshot.memory;
        push_sample(&mut memory.used_percent, used_percent);
        memory.used_bytes = (memory.total_bytes as f32 * used_percent / 100.0) as u64;

        let engines = [
            self.wave(12.0, 10.0, 30.0),
            self.wave(4.0, 4.0, 45.0),
            self.wave(6.0, 2.0, 80.0),
        ];
        let gpu = &mut snapshot.gpu;
        for (history, value) in gpu.engines.iter_mut().zip(engines) {
            push_sample(history, value);
        }
        push_sample(&mut gpu.busy, engines.iter().copied().fold(0.0, f32::max));
        gpu.vram_used_bytes = (2.1 * GIB + engines[0] * 40.0 * MIB) as u64;
        gpu.discrete_asleep = self.tick % 120 < 60;

        let read = self.wave(1.2, 2.0, 18.0) * MIB;
        let write = self.wave(0.6, 1.4, 22.0) * MIB;
        push_sample(&mut snapshot.disk.read, read);
        push_sample(&mut snapshot.disk.write, write);
        snapshot.disk.used_bytes = (412.0 * GIB) as u64;

        let download = self.wave(2.4, 3.0, 12.0) * MIB;
        let upload = self.wave(0.3, 0.4, 16.0) * MIB;
        push_sample(&mut snapshot.network.download, download);
        push_sample(&mut snapshot.network.upload, upload);
        snapshot.network.wifi_signal = (0.7 + 0.2 * self.noise()).min(1.0);

        for app in &mut snapshot.apps.apps {
            for process in &mut app.processes {
                process.cpu_percent =
                    (process.cpu_percent * 0.7 + 0.3 * self.noise() * user / 4.0).max(0.0);
            }
        }
    }
}

fn histories(snapshot: &mut LiveSnapshot) -> impl Iterator<Item = &mut Vec<f32>> {
    let LiveSnapshot {
        cpu,
        memory,
        gpu,
        disk,
        network,
        ..
    } = snapshot;
    [
        &mut cpu.total,
        &mut cpu.user,
        &mut cpu.system,
        &mut cpu.iowait,
        &mut cpu.load,
        &mut cpu.pressure,
        &mut cpu.context_switches_per_second,
        &mut cpu.running_tasks,
        &mut memory.used_percent,
        &mut gpu.busy,
        &mut disk.read,
        &mut disk.write,
        &mut network.download,
        &mut network.upload,
    ]
    .into_iter()
    .chain(gpu.engines.iter_mut())
}

struct DemoApp {
    name: &'static str,
    icon: &'static str,
    processes: &'static [&'static str],
    mark: RowMark,
}

const DEMO_APPS: [DemoApp; 8] = [
    DemoApp {
        name: "Firefox",
        icon: icons::GLOBE,
        processes: &[
            "firefox",
            "Web Content",
            "Web Content",
            "GPU Process",
            "RDD Process",
        ],
        mark: RowMark::None,
    },
    DemoApp {
        name: "Code",
        icon: icons::CODE,
        processes: &["code", "code --type=renderer", "code --type=utility"],
        mark: RowMark::None,
    },
    DemoApp {
        name: "Terminal",
        icon: icons::TERMINAL,
        processes: &["crownterm", "fish", "cargo"],
        mark: RowMark::None,
    },
    DemoApp {
        name: "Files",
        icon: icons::FOLDER,
        processes: &["crownos-files"],
        mark: RowMark::New,
    },
    DemoApp {
        name: "Spotify",
        icon: icons::MUSIC,
        processes: &["spotify", "spotify --type=gpu-process"],
        mark: RowMark::None,
    },
    DemoApp {
        name: "PipeWire",
        icon: icons::AUDIO_LINES,
        processes: &["pipewire", "wireplumber", "pipewire-pulse"],
        mark: RowMark::None,
    },
    DemoApp {
        name: "Steam",
        icon: icons::GAMEPAD_2,
        processes: &["steam", "steamwebhelper"],
        mark: RowMark::Ended,
    },
    DemoApp {
        name: "Kernel",
        icon: icons::CPU,
        processes: &["kworker/0:1", "kswapd0", "ksoftirqd/0"],
        mark: RowMark::None,
    },
];

const FIRST_PID: u32 = 1200;
const USERS: [&str; 2] = ["marvel", "root"];

fn demo_apps() -> AppList {
    let mut pid = FIRST_PID;
    let apps = DEMO_APPS
        .iter()
        .zip(1_u64..)
        .map(|(app, key)| AppEntry {
            key,
            name: app.name.into(),
            icon: app.icon,
            mark: app.mark,
            processes: app
                .processes
                .iter()
                .enumerate()
                .map(|(index, name)| {
                    pid += 7;
                    ProcessEntry {
                        pid,
                        name: (*name).into(),
                        user: USERS[usize::from(app.name == "Kernel")].into(),
                        cpu_percent: 1.0 + index as f32,
                        memory_bytes: (key * 97 + index as u64 * 31) * 1024 * 1024,
                        threads: 4 + index as u32 * 3,
                        mark: app.mark,
                        ..ProcessEntry::default()
                    }
                })
                .collect(),
            ..AppEntry::default()
        })
        .collect();
    AppList { apps }
}

/// Half an hour of synthetic readings, with a suspend gap.
pub fn seeded_snapshot() -> LiveSnapshot {
    seeded_feed().0
}

/// The demo half hour and a step that appends the next synthetic tick to it.
pub fn seeded_feed() -> (LiveSnapshot, impl FnMut(&mut LiveSnapshot)) {
    let mut generator = DemoGenerator::default();
    let mut snapshot = LiveSnapshot::default();
    generator.seed(&mut snapshot);
    (snapshot, move |snapshot: &mut LiveSnapshot| {
        generator.advance(snapshot)
    })
}
