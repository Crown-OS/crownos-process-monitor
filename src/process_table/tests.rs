//! The shared app table driven headlessly: chips, the Ended recently group, keyboard
//! shortcuts, the row menu and the quit confirmation.

use crowntest::{Role, TestApp, role, text};
use monitor_engine::Snapshot;

use super::apps_table;
use super::test_support::{live_with, window};
use crate::app::live::AppList;
use crate::components::app_table::{Column, RowRef};

/// Whether at least one node matches; text often repeats (a panel and a row share a title).
fn shows(app: &mut crowntest::TestApp, query: crowntest::Query) -> bool {
    app.count(query) > 0
}

fn threads_of(apps: &AppList, row: RowRef) -> f64 {
    match row {
        RowRef::App(app) => apps
            .apps
            .get(app)
            .map_or(0.0, |app| f64::from(app.threads())),
        RowRef::Process(app, process) => apps
            .process(app, process)
            .map_or(0.0, |process| f64::from(process.threads)),
    }
}

const COLUMNS: &[Column<AppList>] = &[Column::metric(
    "threads",
    "Threads",
    64.0,
    |apps, row, out| out.push_str(&threads_of(apps, row).to_string()),
    threads_of,
)];

fn table() -> TestApp {
    let mut app = window(live_with(Snapshot::default()), |cx, state| {
        let inspect = cx.signal(None);
        apps_table(cx, state, "Apps", COLUMNS, 0, inspect)
    });
    app.settle();
    app
}

fn rows(app: &mut TestApp) -> Vec<String> {
    let mut rows: Vec<_> = role(Role::ListItem)
        .find_all(app.ui().semantic_tree())
        .into_iter()
        .map(|node| (node.bounds.origin.y, node.name.clone().unwrap_or_default()))
        .collect();
    rows.sort_by(|first, second| first.0.total_cmp(&second.0));
    rows.into_iter().map(|(_, name)| name).collect()
}

fn select(app: &mut TestApp, name: &str) {
    app.click(role(Role::ListItem).name(name));
    app.settle();
}

#[test]
fn ended_recently_is_the_last_group() {
    let mut app = table();
    assert_eq!(rows(&mut app), ["Firefox", "PipeWire", "Ended recently"]);
    app.click(role(Role::Button).name("Expand Ended recently"));
    app.settle();
    assert!(app.exists(role(Role::ListItem).name("make")));
    assert!(
        shows(&mut app, text("peak")),
        "ended rows show their peak memory"
    );
}

#[test]
fn chips_filter_by_category() {
    let mut app = table();
    app.click(role(Role::RadioButton).name("System"));
    app.settle();
    assert_eq!(rows(&mut app), ["PipeWire"]);
    app.click(role(Role::RadioButton).name("My apps"));
    app.settle();
    assert_eq!(rows(&mut app), ["Firefox", "Ended recently"]);
}

#[test]
fn browser_roles_show_beside_the_app() {
    let mut app = table();
    assert!(shows(&mut app, text("1 tab · 1 extension")));
    app.click(role(Role::Button).name("Expand Firefox"));
    app.settle();
    assert!(shows(&mut app, text("Extension")));
}

#[test]
fn ctrl_f_jumps_to_find() {
    let mut app = table();
    select(&mut app, "Firefox");
    app.press_key("Ctrl+F");
    app.settle();
    app.type_text("pipe");
    app.settle();
    assert_eq!(rows(&mut app), ["PipeWire"]);
}

#[test]
fn ctrl_f_finds_without_focusing_the_table_first() {
    let mut app = table();
    app.focus(role(Role::RadioButton).name("All"));
    app.press_key("Ctrl+F");
    app.settle();
    app.type_text("pipe");
    app.settle();
    assert_eq!(rows(&mut app), ["PipeWire"]);
}

#[test]
fn the_menu_key_opens_the_row_menu() {
    let mut app = table();
    select(&mut app, "Firefox");
    assert!(!app.exists(role(Role::Menu)));
    app.press_key("Shift+F10");
    app.settle();
    assert!(app.exists(role(Role::Menu)), "{}", app.semantics_snapshot());
    for item in [
        "Inspect",
        "Copy Name",
        "Copy Path",
        "Quit…",
        "Force Quit…",
        "Renice",
    ] {
        assert!(app.exists(role(Role::MenuItem).name(item)), "no {item}");
    }
}

#[test]
fn quitting_states_how_many_processes_close() {
    let mut app = table();
    select(&mut app, "Firefox");
    app.press_key("Shift+F10");
    app.settle();
    app.click(role(Role::MenuItem).name("Quit…"));
    app.settle();
    assert!(app.exists(role(Role::Heading).name("Quit Firefox?")));
    assert!(shows(&mut app, text("3 processes will close.")));
    app.click(role(Role::Button).name("Cancel"));
    app.settle();
    assert!(!shows(&mut app, text("3 processes will close.")));
}

#[test]
fn delete_asks_before_quitting() {
    let mut app = table();
    select(&mut app, "PipeWire");
    app.press_key("Delete");
    app.settle();
    assert!(shows(&mut app, text("1 process will close.")));
}

#[test]
fn copy_name_puts_it_on_the_clipboard() {
    let mut app = table();
    select(&mut app, "Firefox");
    app.press_key("Shift+F10");
    app.settle();
    app.click(role(Role::MenuItem).name("Copy Name"));
    app.settle();
    let copied = app
        .clipboard()
        .iter()
        .any(|(_, data)| data.as_slice() == b"Firefox");
    assert!(copied);
}

fn four_cpus() -> Snapshot {
    let mut tick = Snapshot::default();
    tick.cpu.logical = (0..4)
        .map(|cpu| monitor_engine::system::LogicalCpuUsage {
            cpu,
            ..Default::default()
        })
        .collect();
    tick
}

#[test]
fn process_rows_offer_cpu_affinity_with_a_switch_per_cpu() {
    let mut app = window(live_with(four_cpus()), |cx, state| {
        let inspect = cx.signal(None);
        apps_table(cx, state, "Apps", COLUMNS, 0, inspect)
    });
    app.settle();
    select(&mut app, "Firefox");
    app.press_key("Shift+F10");
    app.settle();
    assert!(!app.exists(role(Role::MenuItem).name("CPU Affinity…")));
    app.press_key("Escape");
    app.settle();
    app.click(role(Role::Button).name("Expand Firefox"));
    app.settle();
    select(&mut app, "Web Content");
    app.press_key("Shift+F10");
    app.settle();
    app.click(role(Role::MenuItem).name("CPU Affinity…"));
    app.settle();
    assert!(
        app.exists(role(Role::Heading).name("CPU Affinity of Web Content")),
        "{}",
        app.semantics_snapshot()
    );
    for cpu in 0..4 {
        assert!(app.exists(role(Role::Switch).name(format!("CPU {cpu}"))));
    }
    assert!(!app.exists(role(Role::Switch).name("CPU 4")));
}
