use crownui::kit::{
    ButtonOptions, ButtonVariant, icon_button, icons, provide_configured_theme,
    sidebar_with_header, slide_transition, title_bar_spacer, window_controls,
};
use crownui::prelude::{Curve, Cx, Listeners, Styled, View, bind, component, vstack, zstack};

use monitor_engine::EngineControl;

use super::bridge;
use super::live::LiveSnapshot;
use super::navigation::{Navigation, Page};
use super::sidebar::sidebar_menus;
use super::state::AppState;
use crate::pages;
use crate::settings::{MonitorSettings, Settings, SettingsWriter, settings_dialog};

const THEME_FADE_MILLIS: u64 = 240;

/// The whole window. Settings, theme and the engine feed last as long as it does.
pub fn root(initial: MonitorSettings, writer: SettingsWriter, engine: EngineControl) -> impl View {
    component(move |cx| {
        provide_configured_theme(cx);
        let settings = Settings::new(cx, initial, Some(writer));
        let state = AppState::new(cx, settings, LiveSnapshot::default());
        let navigation = Navigation::new(cx, Page::Overview);
        bridge::start(cx, state, navigation, engine);
        shell(cx, state, navigation)
    })
}

/// Everything the window draws over a given state and page.
pub(super) fn shell(cx: &mut Cx, state: AppState, navigation: Navigation) -> impl View + use<> {
    let theme = state.theme;
    let settings_open = cx.signal(false);
    let content = slide_transition(navigation.current(), move |cx, index| {
        pages::build(cx, state, navigation, Page::from_index(index))
    })
    .flex_1();
    let header = (
        title_bar_spacer(),
        icon_button(icons::SETTINGS, "Settings")
            .variant(ButtonVariant::Ghost)
            .on_click(move |cx, _| settings_open.set(cx, true)),
        window_controls(),
    );
    let backdrop = vstack(())
        .w_full()
        .h_full()
        .bg(bind(move |runtime| {
            theme.with(runtime, |theme| theme.surface.bg)
        }))
        .transition(Curve::ease_in_out(THEME_FADE_MILLIS));
    zstack((
        backdrop,
        sidebar_with_header(sidebar_menus(cx, navigation, state), header, content),
        settings_dialog(settings_open, state.settings),
    ))
}
