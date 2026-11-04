use crownui::kit::{
    ButtonOptions, ButtonVariant, ToggleItem, button, dialog, section, setting_row, toggle_group,
};
use crownui::prelude::{
    Cx, Listeners, Role, Semantic, Signal, Styled, View, component, hstack, scroll_view, spacer,
    text, vstack,
};

use super::choice::Choice;
use super::insights_section::insights_section;
use super::model::MonitorSettings;
use super::store::Settings;

const DIALOG_WIDTH: f32 = 520.0;
const DIALOG_MAX_HEIGHT: f32 = 560.0;
const DIALOG_PADDING: f32 = 20.0;
const SECTION_GAP: f32 = 16.0;
const TITLE_SIZE: f32 = 18.0;

/// The settings sheet opened from the header's gear button.
pub fn settings_dialog(open: Signal<bool>, settings: Settings) -> impl View {
    dialog(open, move || {
        component(move |cx| {
            let sections = (
                units_section(cx, settings),
                section(
                    "Sampling",
                    choice_row(
                        cx,
                        settings,
                        "Interval",
                        |current| current.sampling,
                        |current, value| current.sampling = value,
                    ),
                ),
                section(
                    "History",
                    choice_row(
                        cx,
                        settings,
                        "Keep history for",
                        |current| current.retention,
                        |current, value| current.retention = value,
                    ),
                ),
                appearance_section(cx, settings),
                insights_section(cx, settings),
            );
            vstack((
                text("Settings")
                    .text_size(TITLE_SIZE)
                    .font_semibold()
                    .role(Role::Heading),
                scroll_view(vstack(sections).gap(SECTION_GAP))
                    .max_h(DIALOG_MAX_HEIGHT)
                    .label("Settings sections"),
                hstack((
                    spacer(),
                    button("Done")
                        .variant(ButtonVariant::Primary)
                        .on_click(move |cx, _| open.set(cx, false)),
                )),
            ))
            .gap(SECTION_GAP)
            .p(DIALOG_PADDING)
            .w(DIALOG_WIDTH)
            .label("Settings")
        })
    })
}

fn units_section(cx: &mut Cx, settings: Settings) -> impl View + use<> {
    section(
        "Units",
        (
            choice_row(
                cx,
                settings,
                "Temperature",
                |current| current.temperature,
                |current, value| current.temperature = value,
            ),
            choice_row(
                cx,
                settings,
                "Network and disk",
                |current| current.rates,
                |current, value| current.rates = value,
            ),
            choice_row(
                cx,
                settings,
                "CPU usage",
                |current| current.cpu_scale,
                |current, value| current.cpu_scale = value,
            ),
        ),
    )
}

fn appearance_section(cx: &mut Cx, settings: Settings) -> impl View + use<> {
    section(
        "Appearance",
        (
            choice_row(
                cx,
                settings,
                "Palette",
                |current| current.palette,
                |current, value| current.palette = value,
            ),
            choice_row(
                cx,
                settings,
                "Spacing",
                |current| current.spacing,
                |current, value| current.spacing = value,
            ),
            choice_row(
                cx,
                settings,
                "Chart window",
                |current| current.chart_window,
                |current, value| current.chart_window = value,
            ),
        ),
    )
}

pub(super) fn choice_row<C: Choice>(
    cx: &mut Cx,
    settings: Settings,
    label: &'static str,
    get: fn(&MonitorSettings) -> C,
    set: fn(&mut MonitorSettings, C),
) -> impl View + use<C> {
    let selected = settings.choice(cx, get, set);
    let items = C::ALL.iter().map(|option| ToggleItem::text(option.label()));
    setting_row(label, toggle_group(items, selected))
}
