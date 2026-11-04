//! Fixtures for the headless table, page and inspector tests.

use std::sync::Arc;

use crowntest::TestApp;
use crownui::kit::{Theme, icons, provide_theme};
use crownui::prelude::{Cx, Styled, View, component, vstack};
use monitor_core::{AppKey, AppKind, ProcessKey};
use monitor_engine::Snapshot;
use monitor_engine::system::ProcessRole;

use crate::app::AppState;
use crate::app::live::{
    AppEntry, AppFacts, AppList, ENDED_GROUP_KEY, LiveSnapshot, ProcessEntry, ProcessFacts,
};
use crate::components::app_table::{AppCategory, RowMark};
use crate::settings::{MonitorSettings, Settings};

pub const WIDTH: f32 = 1180.0;
pub const HEIGHT: f32 = 2400.0;

fn process(pid: u32, name: &str, role: ProcessRole) -> ProcessEntry {
    ProcessEntry {
        pid,
        name: name.into(),
        user: "marvel".into(),
        cpu_percent: 1.0,
        memory_bytes: 64 << 20,
        threads: 4,
        facts: ProcessFacts {
            start_ticks: 1,
            command_line: format!("/usr/bin/{name} --flag").into(),
            path: Some(format!("/usr/bin/{name}").into()),
            role,
            started_unix: 1_000 + u64::from(pid),
            disk_read: 4096.0,
            download: 1024.0,
            ..ProcessFacts::default()
        },
        ..ProcessEntry::default()
    }
}

fn app(
    key: u64,
    name: &str,
    (kind, category): (AppKind, AppCategory),
    processes: Vec<ProcessEntry>,
) -> AppEntry {
    AppEntry {
        key,
        name: name.into(),
        icon: icons::APP_WINDOW,
        processes,
        facts: AppFacts {
            app_key: Some(AppKey::new(kind, &name.to_lowercase())),
            category,
            pausable: true,
            ..AppFacts::default()
        },
        ..AppEntry::default()
    }
}

/// Firefox with three processes, a service, and the Ended recently group.
pub fn fixture_apps() -> AppList {
    let mut firefox = app(
        1,
        "Firefox",
        (AppKind::Desktop, AppCategory::Mine),
        vec![
            process(4001, "firefox", ProcessRole::Main),
            process(4002, "Web Content", ProcessRole::Tab),
            process(4003, "Web Extensions", ProcessRole::Extension),
        ],
    );
    firefox.facts.roles.add(ProcessRole::Tab);
    firefox.facts.roles.add(ProcessRole::Extension);
    let pipewire = app(
        2,
        "PipeWire",
        (AppKind::Service, AppCategory::System),
        vec![process(5001, "pipewire", ProcessRole::Main)],
    );
    let ended = AppEntry {
        key: ENDED_GROUP_KEY,
        name: "Ended recently".into(),
        icon: icons::HISTORY,
        mark: RowMark::Ended,
        processes: vec![ProcessEntry {
            pid: 6001,
            name: "make".into(),
            mark: RowMark::Ended,
            facts: ProcessFacts {
                started_unix: 3_600,
                ended_unix: 3_660,
                peak_memory: 8 << 20,
                ..ProcessFacts::default()
            },
            ..ProcessEntry::default()
        }],
        facts: AppFacts {
            category: AppCategory::Ended,
            ..AppFacts::default()
        },
    };
    AppList {
        apps: vec![firefox, pipewire, ended],
    }
}

pub fn live_with(tick: Snapshot) -> LiveSnapshot {
    let mut live = LiveSnapshot {
        apps: fixture_apps(),
        tick: Arc::new(tick),
        ..LiveSnapshot::default()
    };
    live.disk.prepare();
    live.network.prepare();
    live
}

/// The test's own process, which it may inspect.
pub fn own_key() -> Option<ProcessKey> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let start_ticks = stat
        .rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()?;
    Some(ProcessKey {
        pid: monitor_core::Pid::new(std::process::id()),
        start_ticks,
    })
}

/// A dark window around whatever `build` makes from an app state over `live`.
pub fn window<V: View>(
    live: LiveSnapshot,
    build: impl FnOnce(&mut Cx, AppState) -> V + 'static,
) -> TestApp {
    TestApp::with_size(
        component(move |cx| {
            provide_theme(cx, Theme::dark());
            let settings = Settings::new(cx, MonitorSettings::default(), None);
            let state = AppState::new(cx, settings, live);
            vstack(build(cx, state)).w(WIDTH).h(HEIGHT)
        }),
        WIDTH,
        HEIGHT,
    )
}
