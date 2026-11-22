//! Mounting the real window over the sample laptop, and pointer helpers for driving it.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::{Context, Result};
use capture::CaptureApp;
use crownos_process_monitor::app::live::LiveSnapshot;
use crownos_process_monitor::app::{
    AppState, EngineLink, Navigation, Page, app_config, replay, shell,
};
use crownos_process_monitor::settings::{MonitorSettings, SETTINGS_SECTION, Settings};
use crowntest::Query;
use crownui::AppConfig;
use crownui::kit::provide_configured_theme;
use crownui::prelude::component;
use monitor_core::util::time::unix_now;
use monitor_engine::{Snapshot, utc_offset_seconds};

use crate::apps::{USER_NAME, USER_UID};
use crate::machine;
use crate::tasks::TaskAnswers;

pub const WINDOW: (f32, f32) = (1180.0, 780.0);
const DAY_SECONDS: i64 = 86_400;
const EPOCH_WEEKDAY: i64 = 3;
const DAILY_GIB: [f32; 7] = [3.1, 4.6, 2.2, 5.8, 3.9, 1.7, 2.9];
const GIB: f32 = (1_u64 << 30) as f32;
const AVERAGE_CPU_TODAY: f32 = 18.4;
const FRAME_MILLIS: u64 = 16;
const BACKGROUND_ROUNDS: usize = 8;
const BACKGROUND_PAUSE: std::time::Duration = std::time::Duration::from_millis(25);

/// What every mount shares: the ticks, the sample history and the real window config.
pub struct Fixture {
    ticks: Vec<Arc<Snapshot>>,
    history: PathBuf,
    config: AppConfig,
    settings: MonitorSettings,
}

/// The mounted window and the handles its state is driven through.
pub struct Window {
    pub app: CaptureApp,
    pub state: AppState,
    tasks: TaskAnswers,
}

/// How a mount departs from the plain sample laptop.
#[derive(Clone, Copy, Default)]
pub enum Machine {
    #[default]
    Laptop,
    NoGpu,
}

impl Fixture {
    pub fn new() -> Result<Self> {
        let design = std::env::var_os("CROWN_DESIGN_DIR").map_or_else(
            || std::env::temp_dir().join("process-monitor-design"),
            PathBuf::from,
        );
        Ok(Self {
            ticks: machine::ticks(),
            history: crate::history::seed(&design.join(".history"))?,
            config: app_config(),
            settings: crownconfig::load_section(SETTINGS_SECTION),
        })
    }

    /// The replayed half hour, its last tick carrying the page as the engine's interest.
    fn live(&self, page: Page, machine: Machine) -> LiveSnapshot {
        let (last, earlier) = self
            .ticks
            .split_last()
            .map_or((None, &[][..]), |(last, earlier)| (Some(last), earlier));
        let last = last.map(|tick| {
            let mut tick = (**tick).clone();
            tick.interest.page = engine_page(page);
            if let Machine::NoGpu = machine {
                tick.gpu = Default::default();
            }
            Arc::new(tick)
        });
        let ticks = earlier.iter().cloned().chain(last);
        let mut live = replay(ticks, self.settings.cpu_scale);
        let user: Arc<str> = USER_NAME.into();
        for process in live
            .apps
            .apps
            .iter_mut()
            .flat_map(|app| app.processes.iter_mut())
        {
            if process.facts.uid == USER_UID {
                process.user = Arc::clone(&user);
            }
        }
        let today = unix_now() + i64::from(utc_offset_seconds());
        let first_day = today.div_euclid(DAY_SECONDS) - (DAILY_GIB.len() as i64 - 1);
        live.network.daily_bytes = DAILY_GIB.iter().map(|gib| gib * GIB).collect();
        live.network.daily_first_weekday = (first_day + EPOCH_WEEKDAY).rem_euclid(7) as usize;
        live.cpu.average_today = Some(AVERAGE_CPU_TODAY);
        live
    }

    /// The newest `count` ticks, oldest first.
    pub fn recent(&self, count: usize) -> &[Arc<Snapshot>] {
        &self.ticks[self.ticks.len().saturating_sub(count)..]
    }

    /// The window as the binary builds it, on `page`, over the sample laptop.
    pub fn open(&self, page: Page, size: Option<(f32, f32)>, machine: Machine) -> Result<Window> {
        let mounted = Rc::new(Cell::new(None));
        let handle = Rc::clone(&mounted);
        let mut live = Some(self.live(page, machine));
        let history = self.history.clone();
        let settings = self.settings;
        let root = component(move |cx| {
            provide_configured_theme(cx);
            EngineLink::offline(history.clone(), crate::inspect::inspect).provide(cx);
            let settings = Settings::new(cx, settings, None);
            let state = AppState::new(cx, settings, live.take().unwrap_or_default());
            let navigation = Navigation::new(cx, page);
            handle.set(Some(state));
            shell(cx, state, navigation)
        });
        let mut app = capture::mount_app(root, &self.config, Some(size.unwrap_or(WINDOW)))?;
        let state = mounted.get().context("the window did not mount")?;
        app.pointer_move(1.0, 1.0);
        let tasks = TaskAnswers::install(app.ui())?;
        let mut window = Window { app, state, tasks };
        window.settle();
        Ok(window)
    }
}

const fn engine_page(page: Page) -> monitor_core::Page {
    match page {
        Page::Overview => monitor_core::Page::Overview,
        Page::Cpu => monitor_core::Page::Cpu,
        Page::Gpu => monitor_core::Page::Gpu,
        Page::Memory => monitor_core::Page::Memory,
        Page::Disk => monitor_core::Page::Disk,
        Page::Network => monitor_core::Page::Network,
    }
}

impl Window {
    /// Lets background work (history queries, assessments, inspections) answer, then settles.
    pub fn settle(&mut self) -> &mut Self {
        for _ in 0..BACKGROUND_ROUNDS {
            std::thread::sleep(BACKGROUND_PAUSE);
            self.tasks.deliver(self.app.ui());
            self.app.advance_ms(BACKGROUND_PAUSE.as_millis() as u64);
        }
        self.app.settle();
        self
    }

    /// Presses and releases the primary button over the node, as a real click does, so
    /// pointer-positioned menus open where it was pressed.
    pub fn press(&mut self, query: Query) -> &mut Self {
        let center = self.app.find(query).bounds.center();
        self.app.pointer_move(center.x, center.y);
        self.app.pointer_down(center.x, center.y);
        self.app.pointer_up(center.x, center.y);
        self.settle()
    }

    /// Feeds `ticks` one frame apart, as the engine would, so per-process series fill.
    pub fn stream(&mut self, ticks: &[Arc<Snapshot>]) -> &mut Self {
        let live = self.state.live;
        for tick in ticks {
            live.update(self.app.ui().cx(), |snapshot| {
                snapshot.tick = Arc::clone(tick)
            });
            self.app.advance_ms(FRAME_MILLIS);
        }
        self.settle()
    }

    /// Selects a table row with the pointer and focuses it, as a click does for the keyboard.
    pub fn select(&mut self, query: Query) -> &mut Self {
        self.press(query.clone());
        self.click(query)
    }

    pub fn click(&mut self, query: Query) -> &mut Self {
        self.app.click(query);
        self.settle()
    }

    pub fn key(&mut self, shortcut: &str) -> &mut Self {
        self.app.press_key(shortcut);
        self.settle()
    }

    /// Scrolls the page so the node's top edge sits `margin` below the window top.
    pub fn scroll_to(&mut self, target: Query, margin: f32) -> &mut Self {
        let top = self.app.find(target.clone()).bounds.origin.y;
        self.app.scroll(target, top - margin);
        self.settle()
    }

    /// The lowest edge of anything inside the page's scroll view, for full-length captures.
    pub fn content_bottom(&mut self, page: &str) -> f32 {
        let tree = self.app.ui().semantic_tree();
        let scroll = crowntest::role(crowntest::Role::ScrollView).name(page);
        let Ok(root) = scroll.find_one(tree) else {
            return 0.0;
        };
        let mut pending = tree
            .get(root)
            .map(|node| node.children.clone())
            .unwrap_or_default();
        let mut bottom: f32 = 0.0;
        while let Some(id) = pending.pop() {
            if let Some(node) = tree.get(id) {
                bottom = bottom.max(node.bounds.origin.y + node.bounds.size.height);
                pending.extend(node.children.iter().copied());
            }
        }
        bottom
    }
}
