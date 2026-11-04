//! The drawer over the test's own process, read synchronously so no task has to finish.

use std::sync::Arc;

use crowntest::{Role, role, text};
use crownui::prelude::{Styled, vstack};
use monitor_engine::Snapshot;
use monitor_engine::system::inspect::inspect;

use super::{Inspector, InspectorData, Loaded, inspector_drawer};
use crate::process_table::test_support::{live_with, own_key, window};

/// Whether at least one node matches; text often repeats (a panel and a row share a title).
fn shows(app: &mut crowntest::TestApp, query: crowntest::Query) -> bool {
    app.count(query) > 0
}

#[test]
fn the_drawer_shows_every_section_of_our_own_process() {
    let Some(key) = own_key() else {
        return;
    };
    let Ok(details) = inspect(key) else {
        return;
    };
    let mut app = window(live_with(Snapshot::default()), move |cx, state| {
        let inspector = Inspector::new(cx);
        inspector.data.set(
            cx,
            InspectorData {
                revision: 1,
                loaded: Loaded::Ready(Arc::new(details)),
            },
        );
        inspector.target.set(cx, Some(key));
        inspector_drawer(cx, state, inspector, vstack(()).w_full())
    });
    app.settle();
    for section in [
        "Identity",
        "Executable",
        "Security",
        "Resource usage",
        "Environment",
    ] {
        assert!(
            app.exists(role(Role::Heading).name(section)),
            "no {section} section:\n{}",
            app.semantics_snapshot()
        );
    }
    assert!(app.exists(role(Role::Group).name("PID")));
    assert!(
        shows(&mut app, text("Show variables")),
        "the environment starts folded"
    );
    assert!(app.exists(role(Role::Button).name("Save as CSV")));
    app.click(role(Role::Button).name("Copy as Text"));
    app.settle();
    let copied = app
        .clipboard()
        .iter()
        .any(|(_, data)| String::from_utf8_lossy(data).contains("Identity"));
    assert!(copied, "the export lands on the clipboard");
}

#[test]
fn closing_the_drawer_clears_the_target() {
    let Some(key) = own_key() else {
        return;
    };
    let mut app = window(live_with(Snapshot::default()), move |cx, state| {
        let inspector = Inspector::new(cx);
        inspector.target.set(cx, Some(key));
        let target = inspector.target;
        vstack((
            inspector_drawer(cx, state, inspector, vstack(()).w_full()),
            crownui::prelude::text(crownui::prelude::bind(move |runtime| {
                format!("open {}", target.get(runtime).is_some())
            })),
        ))
    });
    app.settle();
    assert!(shows(&mut app, text("Reading the process")));
    app.click(role(Role::Button).name("Close"));
    app.settle();
    assert!(shows(&mut app, text("open false")));
}
