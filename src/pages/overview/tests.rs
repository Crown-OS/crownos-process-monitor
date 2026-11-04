//! The Overview headless over a seeded tick.

use crowntest::{Role, TestApp, role};
use crownui::kit::{Theme, provide_theme};
use crownui::prelude::{Styled, bind, component, text, vstack};

use super::fixtures::seeded_live;
use crate::app::{AppState, Navigation, Page};
use crate::settings::{MonitorSettings, Settings};

const WIDTH: f32 = 1180.0;
const HEIGHT: f32 = 3200.0;

fn overview() -> TestApp {
    TestApp::with_size(
        component(|cx| {
            provide_theme(cx, Theme::dark());
            let settings = Settings::new(cx, MonitorSettings::default(), None);
            let state = AppState::new(cx, settings, seeded_live());
            let navigation = Navigation::new(cx, Page::Overview);
            let current = navigation.current();
            vstack((
                text(bind(move |runtime| {
                    format!("Showing {}", Page::from_index(current.get(runtime)).title())
                })),
                super::page(cx, state, navigation),
            ))
            .w(WIDTH)
            .h(HEIGHT)
        }),
        WIDTH,
        HEIGHT,
    )
}

fn shows_text(app: &mut TestApp, wanted: &str) -> bool {
    app.ui().semantic_tree().iter().any(|node| {
        node.name
            .as_deref()
            .is_some_and(|name| name.contains(wanted))
    })
}

#[test]
fn only_the_cards_render_over_seeded_data() {
    let mut app = overview();
    app.settle();
    for card in ["CPU", "Memory", "Disk", "Network", "Power", "Sensors"] {
        assert!(app.exists(role(Role::Button).name(card)), "no {card} card");
    }
    for removed in ["Over Time", "Memory by Type", "Worth a Look", "processes"] {
        assert!(
            !shows_text(&mut app, removed),
            "{removed} is still shown:\n{}",
            app.semantics_snapshot()
        );
    }
}

#[test]
fn a_card_click_opens_its_page() {
    let mut app = overview();
    app.settle();
    assert!(shows_text(&mut app, "Showing Overview"));
    app.click(role(Role::Button).name("Sensors"));
    app.settle();
    assert!(shows_text(&mut app, "Showing CPU"));
    app.click(role(Role::Button).name("Network"));
    app.settle();
    assert!(shows_text(&mut app, "Showing Network"));
}
