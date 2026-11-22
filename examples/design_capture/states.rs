//! Every state, grouped as the Figma page shows them.

use anyhow::Result;
use capture::Carrier;
use crownos_process_monitor::app::Page;
use crownos_process_monitor::settings::{
    InsightSettings, LimitValue, QuietTarget, SavedApp, SavedLimit, SavedQuiet,
};
use crowntest::{Role, role};
use monitor_core::util::time::unix_now;
use monitor_core::{AppKey, AppKind};
use monitor_engine::alerts::AlertKind;
use monitor_engine::utc_offset_seconds;

use crate::window::{Fixture, Machine, WINDOW, Window};

/// One section of the Figma page.
pub type Group = fn(&Fixture, &mut Carrier) -> Result<()>;

const FULL_PAGE_PADDING: f32 = 32.0;
const TABLE_MARGIN: f32 = 150.0;
const GIB: u64 = 1 << 30;
const DAY_SECONDS: i64 = 86_400;
const INSPECTOR_SAMPLES: usize = 120;
/// Wide enough that the inspector drawer sits beside the table without overflowing.
const WIDE_WINDOW: (f32, f32) = (1600.0, 900.0);

const PAGES: [(Page, &str); 6] = [
    (
        Page::Overview,
        "Every subsystem at a glance; cards open their page",
    ),
    (
        Page::Cpu,
        "Usage strip, hardware, utilization chart and the top of the page",
    ),
    (
        Page::Gpu,
        "The integrated Radeon selected, with engines and VRAM",
    ),
    (Page::Memory, "Usage strip, category breakdown and swap"),
    (Page::Disk, "Throughput, totals, volumes and drives"),
    (Page::Network, "Traffic, totals, interfaces and Wi-Fi"),
];

pub fn pages(fixture: &Fixture, carrier: &mut Carrier) -> Result<()> {
    for (page, description) in PAGES {
        let mut window = fixture.open(page, None, Machine::Laptop)?;
        carrier.capture_state(&mut window.app, "Pages", page.title(), description)?;
    }
    Ok(())
}

pub fn full_pages(fixture: &Fixture, carrier: &mut Carrier) -> Result<()> {
    for (page, _) in PAGES {
        let bottom = {
            let mut window = fixture.open(page, None, Machine::Laptop)?;
            window.content_bottom(page.title())
        };
        let height = (bottom + FULL_PAGE_PADDING).max(WINDOW.1).ceil();
        let mut window = fixture.open(page, Some((WINDOW.0, height)), Machine::Laptop)?;
        carrier.capture_state(
            &mut window.app,
            "Full-length pages",
            &format!("{} (full)", page.title()),
            "The whole scrolling page in one frame, every panel laid out",
        )?;
    }
    Ok(())
}

/// The CPU page scrolled so its Apps table fills the window.
fn cpu_table(fixture: &Fixture) -> Result<Window> {
    let mut window = fixture.open(Page::Cpu, None, Machine::Laptop)?;
    window.scroll_to(role(Role::TextField).name("Find"), TABLE_MARGIN);
    Ok(window)
}

fn row(name: &str) -> crowntest::Query {
    role(Role::ListItem).name(name)
}

/// Narrows the table to My apps, so Ended recently fits, and expands it.
fn reveal_ended(window: &mut Window) {
    window.click(role(Role::RadioButton).name("My apps"));
    window.click(role(Role::Button).name("Expand Ended recently"));
}

pub fn table(fixture: &Fixture, carrier: &mut Carrier) -> Result<()> {
    let group = "Apps table";
    let mut window = cpu_table(fixture)?;
    carrier.capture_state(
        &mut window.app,
        group,
        "Sorted by CPU",
        "The default order: busiest app first",
    )?;

    window.select(row("Terminal"));
    carrier.capture_state(
        &mut window.app,
        group,
        "Selected row",
        "Terminal selected with the pointer",
    )?;

    let mut window = cpu_table(fixture)?;
    window.click(role(Role::Button).name("Sort by Name"));
    carrier.capture_state(
        &mut window.app,
        group,
        "Sorted by name",
        "Name header clicked: A to Z",
    )?;

    let mut window = cpu_table(fixture)?;
    window.click(role(Role::Button).name("Expand Firefox"));
    window.select(row("GPU Process"));
    carrier.capture_state(
        &mut window.app,
        group,
        "Expanded app",
        "Firefox expanded into its processes, one selected",
    )?;

    let mut window = cpu_table(fixture)?;
    reveal_ended(&mut window);
    carrier.capture_state(
        &mut window.app,
        group,
        "Ended recently",
        "The group of processes that exited, with their peak memory",
    )?;

    let mut window = cpu_table(fixture)?;
    window.click(role(Role::RadioButton).name("System"));
    carrier.capture_state(
        &mut window.app,
        group,
        "System filter",
        "The System chip: services and kernel threads",
    )?;

    let mut window = cpu_table(fixture)?;
    window.click(role(Role::TextField).name("Find"));
    window.app.type_text("rust");
    window.app.settle();
    carrier.capture_state(
        &mut window.app,
        group,
        "Find",
        "Typing narrows the table to matching apps and processes",
    )?;

    let mut window = cpu_table(fixture)?;
    window.click(role(Role::TextField).name("Find"));
    window.app.type_text("blender");
    window.app.settle();
    carrier.capture_state(
        &mut window.app,
        "Empty & error states",
        "No matches",
        "Find with nothing matching",
    )?;

    let mut window = cpu_table(fixture)?;
    window.press(role(Role::Button).name("Columns"));
    carrier.capture_state(
        &mut window.app,
        "Menus",
        "Columns menu",
        "Optional columns, shown ones checked",
    )?;
    Ok(())
}

fn row_menu(fixture: &Fixture, name: &str, expand: Option<&str>) -> Result<Window> {
    let mut window = cpu_table(fixture)?;
    match expand {
        Some("Ended recently") => reveal_ended(&mut window),
        Some(app) => {
            window.click(role(Role::Button).name(format!("Expand {app}")));
        }
        None => {}
    }
    window.select(row(name));
    window.key("Shift+F10");
    Ok(window)
}

pub fn menus(fixture: &Fixture, carrier: &mut Carrier) -> Result<()> {
    let group = "Menus";
    let mut window = row_menu(fixture, "Firefox", None)?;
    carrier.capture_state(
        &mut window.app,
        group,
        "App row menu",
        "Inspect, copy, pin, quit, pause, renice and alerts",
    )?;

    let mut window = row_menu(fixture, "GPU Process", Some("Firefox"))?;
    carrier.capture_state(
        &mut window.app,
        group,
        "Process row menu",
        "A single process adds CPU Affinity",
    )?;

    window.press(role(Role::MenuItem).name("Renice"));
    carrier.capture_state(
        &mut window.app,
        group,
        "Renice submenu",
        "Priority levels, the current one checked",
    )?;

    let mut window = row_menu(fixture, "make", Some("Ended recently"))?;
    carrier.capture_state(
        &mut window.app,
        group,
        "Ended row menu",
        "An ended process can only be copied",
    )?;
    Ok(())
}

fn confirm(fixture: &Fixture, name: &str, item: &str) -> Result<Window> {
    let mut window = row_menu(fixture, name, None)?;
    window.click(role(Role::MenuItem).name(item));
    Ok(window)
}

fn seed_insights(window: &mut Window) {
    let firefox = SavedApp::new(&AppKey::new(AppKind::Desktop, "firefox"), "Firefox");
    let code = SavedApp::new(&AppKey::new(AppKind::Desktop, "code"), "Code");
    let offset = i64::from(utc_offset_seconds());
    let tomorrow = ((unix_now() + offset) / DAY_SECONDS + 1) * DAY_SECONDS - offset;
    let insights = InsightSettings {
        custom_limits: vec![
            SavedLimit {
                app: firefox,
                value: LimitValue::MemoryBytes(4 * GIB),
                minutes: 5,
            },
            SavedLimit {
                app: code.clone(),
                value: LimitValue::CpuPercent(60.0),
                minutes: 15,
            },
        ],
        quieted: vec![
            SavedQuiet {
                target: QuietTarget::kind(AlertKind::MachineHot),
                until_unix: None,
            },
            SavedQuiet {
                target: QuietTarget::App(code),
                until_unix: Some(tomorrow),
            },
        ],
    };
    window
        .state
        .settings
        .insights()
        .set(window.app.ui().cx(), insights);
    window.app.settle();
}

pub fn dialogs(fixture: &Fixture, carrier: &mut Carrier) -> Result<()> {
    let group = "Dialogs";
    let mut window = confirm(fixture, "Firefox", "Quit…")?;
    carrier.capture_state(
        &mut window.app,
        group,
        "Quit confirmation",
        "States how many processes close; Force Quit beside it",
    )?;

    let mut window = confirm(fixture, "Firefox", "Force Quit…")?;
    carrier.capture_state(
        &mut window.app,
        group,
        "Force quit confirmation",
        "Unsaved work is lost",
    )?;

    let mut window = confirm(fixture, "Terminal", "Pause…")?;
    carrier.capture_state(
        &mut window.app,
        group,
        "Pause confirmation",
        "The app's scope freezes until resumed",
    )?;

    let mut window = cpu_table(fixture)?;
    window.click(role(Role::RadioButton).name("System"));
    window.select(row("Kernel"));
    window.key("Shift+F10");
    window.click(role(Role::MenuItem).name("Quit…"));
    carrier.capture_state(
        &mut window.app,
        group,
        "Refused action",
        "Kernel threads cannot be quit; the dialog explains why",
    )?;

    let mut window = row_menu(fixture, "GPU Process", Some("Firefox"))?;
    window.click(role(Role::MenuItem).name("CPU Affinity…"));
    carrier.capture_state(
        &mut window.app,
        group,
        "CPU affinity",
        "A switch per logical CPU",
    )?;

    let mut window = fixture.open(Page::Overview, None, Machine::Laptop)?;
    window.click(role(Role::Button).name("Settings"));
    carrier.capture_state(
        &mut window.app,
        group,
        "Settings",
        "Units, sampling, history and appearance",
    )?;

    let mut window = fixture.open(Page::Overview, None, Machine::Laptop)?;
    seed_insights(&mut window);
    window.click(role(Role::Button).name("Settings"));
    window.scroll_to(role(Role::Button).name("Delete History…"), 380.0);
    carrier.capture_state(
        &mut window.app,
        group,
        "Settings · Insights",
        "Alert limits, per-app limits and quieted alerts",
    )?;

    window.click(role(Role::Button).name("Delete History…"));
    window.scroll_to(role(Role::Button).name("Delete"), 560.0);
    carrier.capture_state(
        &mut window.app,
        group,
        "Delete history",
        "Deleting history is confirmed in place",
    )?;
    Ok(())
}

pub fn inspector(fixture: &Fixture, carrier: &mut Carrier) -> Result<()> {
    let group = "Inspector";
    let mut window = fixture.open(Page::Cpu, Some(WIDE_WINDOW), Machine::Laptop)?;
    window.scroll_to(role(Role::TextField).name("Find"), TABLE_MARGIN);
    window.select(row("Firefox"));
    window.key("Enter");
    window.stream(fixture.recent(INSPECTOR_SAMPLES));
    carrier.capture_state(
        &mut window.app,
        group,
        "Inspector · wide window",
        "The drawer with room beside the table: identity, executable, security, resources",
    )?;

    let mut window = cpu_table(fixture)?;
    window.select(row("Firefox"));
    window.key("Enter");
    window.stream(fixture.recent(INSPECTOR_SAMPLES));
    carrier.capture_state(
        &mut window.app,
        group,
        "Inspector",
        "The drawer over Firefox's main process",
    )?;

    window.click(role(Role::Button).name("Copy as Text"));
    carrier.capture_state(
        &mut window.app,
        "Toasts",
        "Copied",
        "The toast after copying the process details",
    )?;

    let mut window = row_menu(fixture, "rust-analyzer", Some("Code"))?;
    window.click(role(Role::MenuItem).name("Inspect"));
    window.stream(fixture.recent(INSPECTOR_SAMPLES));
    carrier.capture_state(
        &mut window.app,
        group,
        "Inspector · process",
        "A helper process opened from the row menu",
    )?;
    Ok(())
}

pub fn history_and_devices(fixture: &Fixture, carrier: &mut Carrier) -> Result<()> {
    let mut window = fixture.open(Page::Cpu, None, Machine::Laptop)?;
    window.press(role(Role::RadioButton).name("History"));
    carrier.capture_state(
        &mut window.app,
        "History",
        "CPU history · 1 h",
        "History mode: the stored last hour",
    )?;

    let mut window = fixture.open(Page::Network, None, Machine::Laptop)?;
    window.press(role(Role::RadioButton).name("History"));
    carrier.capture_state(
        &mut window.app,
        "History",
        "Network history · 1 h",
        "Stored download and upload",
    )?;

    let mut window = fixture.open(Page::Gpu, None, Machine::Laptop)?;
    window.click(role(Role::RadioButton).name("GeForce RTX 4060 Laptop GPU"));
    carrier.capture_state(
        &mut window.app,
        "Devices",
        "Discrete GPU asleep",
        "The second GPU selected while it sleeps",
    )?;

    let mut window = fixture.open(Page::Disk, None, Machine::Laptop)?;
    window.scroll_to(role(Role::Button).name("Unmount"), 300.0);
    window.click(role(Role::Button).name("Unmount"));
    carrier.capture_state(
        &mut window.app,
        "Dialogs",
        "Unmount confirmation",
        "Removable volumes ask before unmounting",
    )?;

    let mut window = fixture.open(Page::Gpu, None, Machine::NoGpu)?;
    carrier.capture_state(
        &mut window.app,
        "Empty & error states",
        "No GPU",
        "A machine without a GPU explains itself",
    )?;
    Ok(())
}
