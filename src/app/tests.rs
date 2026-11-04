//! The window driven headlessly over the demo half hour: no config files, no ticking.

use crowntest::{Query, Role, TestApp, role};
use crownui::kit::{Theme, provide_theme};
use crownui::prelude::{Styled, component, vstack};

use std::cell::Cell;
use std::rc::Rc;

use super::demo::{seeded_feed, seeded_snapshot};
use super::live::{AppList, LiveSnapshot, push_sample};
use super::navigation::{Navigation, Page};
use super::root::shell;
use super::state::AppState;
use crate::components::app_table::{
    Column, ColumnSet, RowRef, SortColumn, SortDirection, SortOrder, TableState, app_table,
};
use crate::settings::{MonitorSettings, Settings};

const WIDTH: f32 = 1180.0;
const HEIGHT: f32 = 780.0;

fn window(page: Page) -> TestApp {
    TestApp::with_size(
        component(move |cx| {
            provide_theme(cx, Theme::dark());
            let settings = Settings::new(cx, MonitorSettings::default(), None);
            let state = AppState::new(cx, settings, seeded_snapshot());
            let navigation = Navigation::new(cx, page);
            shell(cx, state, navigation)
        }),
        WIDTH,
        HEIGHT,
    )
}

/// Names of every node `query` matches, top to bottom on screen.
fn names_top_down(app: &mut TestApp, query: &Query) -> Vec<String> {
    let mut nodes: Vec<_> = query
        .find_all(app.ui().semantic_tree())
        .into_iter()
        .map(|node| (node.bounds.origin.y, node.name.clone().unwrap_or_default()))
        .collect();
    nodes.sort_by(|first, second| first.0.total_cmp(&second.0));
    nodes.into_iter().map(|(_, name)| name).collect()
}

#[test]
fn the_sidebar_lists_the_six_pages_in_order() {
    let mut app = window(Page::Overview);
    let rows = names_top_down(&mut app, &role(Role::MenuItem));
    assert_eq!(rows.len(), Page::ALL.len(), "{rows:?}");
    for (row, page) in rows.iter().zip(Page::ALL) {
        assert!(row.starts_with(page.title()), "{row:?} is not {page:?}");
    }
    assert_eq!(rows[1], "CPU 27%", "rows carry their live value");
}

#[test]
fn every_page_opens_from_the_sidebar() {
    let mut app = window(Page::Overview);
    for page in Page::ALL.into_iter().rev() {
        app.click(role(Role::MenuItem).text(format!("{} ", page.title())));
        app.settle();
        assert!(
            app.exists(role(Role::ScrollView).name(page.title())),
            "{} did not open:\n{}",
            page.title(),
            app.semantics_snapshot()
        );
    }
}

#[test]
fn ctrl_digits_open_the_pages() {
    let mut app = window(Page::Overview);
    app.focus(role(Role::Button).name("Settings"));
    for (position, page) in Page::ALL.into_iter().enumerate().rev() {
        app.press_key(&format!("Ctrl+{}", position + 1));
        app.settle();
        assert!(
            app.exists(role(Role::ScrollView).name(page.title())),
            "Ctrl+{} did not open {}",
            position + 1,
            page.title()
        );
    }
}

#[test]
fn the_gear_opens_the_settings_dialog() {
    let mut app = window(Page::Overview);
    assert!(!app.exists(role(Role::Heading).name("Settings")));
    app.click(role(Role::Button).name("Settings"));
    app.settle();
    assert!(app.exists(role(Role::Heading).name("Settings")));
    app.click(role(Role::Button).name("Done"));
    app.settle();
    assert!(!app.exists(role(Role::Heading).name("Settings")));
}

#[test]
fn settings_choices_write_the_live_settings() {
    let mut app = window(Page::Cpu);
    app.click(role(Role::RadioButton).name("15 min"));
    app.click(role(Role::Button).name("Settings"));
    app.settle();
    app.click(role(Role::RadioButton).name("Monochrome"));
    app.settle();
    let checked = |app: &mut TestApp, name: &str| {
        role(Role::RadioButton)
            .name(name)
            .find_all(app.ui().semantic_tree())
            .iter()
            .filter(|node| node.states.checked == Some(true))
            .count()
    };
    assert_eq!(
        checked(&mut app, "15 min"),
        2,
        "the range picker's window is the setting the dialog shows"
    );
    assert_eq!(checked(&mut app, "Monochrome"), 1);
}

#[test]
fn overview_cards_open_their_page() {
    let mut app = window(Page::Overview);
    app.click(role(Role::Button).name("Memory"));
    app.settle();
    assert!(app.exists(role(Role::ScrollView).name("Memory")));
}

fn cpu_of(apps: &AppList, row: RowRef) -> f64 {
    let percent = match row {
        RowRef::App(app) => apps.apps[app].cpu_percent(),
        RowRef::Process(app, process) => apps.apps[app].processes[process].cpu_percent,
    };
    f64::from(percent)
}

const TEST_COLUMNS: &[Column<AppList>] = &[Column::metric(
    "cpu",
    "CPU",
    72.0,
    |apps, row, out| out.push_str(&format!("{:.1}", cpu_of(apps, row))),
    cpu_of,
)];

fn apps_of(snapshot: &LiveSnapshot) -> &AppList {
    &snapshot.apps
}

fn table_window() -> TestApp {
    TestApp::with_size(
        component(|cx| {
            provide_theme(cx, Theme::dark());
            let live = cx.signal(seeded_snapshot());
            let columns = cx.signal(ColumnSet::NONE);
            let state = TableState::new(
                cx,
                SortOrder::new(SortColumn::Metric(0), SortDirection::Descending),
                columns,
            );
            vstack(app_table("Apps", live, apps_of, TEST_COLUMNS, state))
                .w(800.0)
                .h(640.0)
        }),
        900.0,
        700.0,
    )
}

#[test]
fn the_app_table_sorts_by_name_both_ways() {
    let mut app = table_window();
    app.click(role(Role::Button).name("Sort by Name"));
    app.settle();
    let ascending = names_top_down(&mut app, &role(Role::ListItem));
    assert_eq!(
        ascending,
        [
            "Code", "Files", "Firefox", "Kernel", "PipeWire", "Spotify", "Steam", "Terminal"
        ]
    );
    assert_eq!(
        app.find(role(Role::Button).name("Sort by Name"))
            .value
            .as_deref(),
        Some("ascending")
    );
    app.click(role(Role::Button).name("Sort by Name"));
    app.settle();
    let descending = names_top_down(&mut app, &role(Role::ListItem));
    assert_eq!(descending, ascending.into_iter().rev().collect::<Vec<_>>());
}

#[test]
fn expanding_an_app_reveals_its_processes() {
    let mut app = table_window();
    assert!(!app.exists(role(Role::ListItem).name("GPU Process")));
    app.click(role(Role::Button).name("Expand Firefox"));
    app.settle();
    assert!(app.exists(role(Role::ListItem).name("GPU Process")));
    assert_eq!(app.count(role(Role::ListItem).name("Web Content")), 2);
    app.click(role(Role::Button).name("Collapse Firefox"));
    app.settle();
    assert!(!app.exists(role(Role::ListItem).name("GPU Process")));
}

#[test]
fn find_narrows_the_table_with_a_regex() {
    let mut app = table_window();
    app.click(role(Role::TextField).name("Find"));
    app.type_text("/^(code|steam)$/");
    app.settle();
    assert_eq!(
        names_top_down(&mut app, &role(Role::ListItem)),
        ["Code", "Steam"]
    );
}

#[test]
fn a_tick_repaints_in_one_frame_on_every_page() {
    const TICKS: usize = 8;
    const TWO_FRAMES_MILLIS: u64 = 34;
    const TICK_MILLIS: u64 = 1000;
    for page in Page::ALL {
        let (snapshot, mut advance) = seeded_feed();
        let mounted = Rc::new(Cell::new(None));
        let handle = Rc::clone(&mounted);
        let mut app = TestApp::with_size(
            component(move |cx| {
                provide_theme(cx, Theme::dark());
                let settings = Settings::new(cx, MonitorSettings::default(), None);
                let state = AppState::new(cx, settings, snapshot);
                handle.set(Some(state));
                let navigation = Navigation::new(cx, page);
                shell(cx, state, navigation)
            }),
            WIDTH,
            HEIGHT,
        );
        app.settle();
        let Some(state) = mounted.get() else {
            panic!("the shell did not mount");
        };
        for tick in 0..TICKS {
            let (rate, percent) = if tick % 2 == 0 {
                (9.0, 5.0)
            } else {
                (98_765_432.0, 55.5)
            };
            state.live.update(app.ui().cx(), |live| {
                advance(live);
                push_sample(&mut live.disk.read, rate);
                push_sample(&mut live.network.download, rate);
                push_sample(&mut live.cpu.total, percent);
            });
            app.advance_ms(TWO_FRAMES_MILLIS);
            assert!(
                !app.is_animating(),
                "{page:?}: tick {tick} is still animating two frames later"
            );
            app.advance_ms(TICK_MILLIS - TWO_FRAMES_MILLIS);
        }
    }
}
