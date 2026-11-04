use crownui::ext::ClickEvent;
use crownui::kit::{ButtonOptions, ButtonVariant, button, dialog, use_theme};
use crownui::prelude::{
    Cx, Event, Listeners, Role, Semantic, Signal, Styled, View, bind, component, hstack, spacer,
    text, vstack,
};

use crate::util::spacing;

const DIALOG_WIDTH: f32 = 380.0;
const DIALOG_PADDING: f32 = 20.0;
const TITLE_SIZE: f32 = 16.0;

/// Which button closed a confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmChoice {
    Confirm,
    Secondary,
}

/// The words on a confirmation: "Quit Firefox?", "96 processes will close.", Quit, Force Quit.
/// Every part is a signal, so one dialog serves several actions and can turn into a refusal
/// (only Cancel left) when the action turns out not to be allowed.
#[derive(Debug, Clone, Copy)]
pub struct Confirmation {
    pub title: Signal<String>,
    pub message: Signal<String>,
    pub confirm_label: Signal<&'static str>,
    pub secondary_label: Signal<Option<&'static str>>,
    /// While false the confirm buttons are disabled and Cancel reads "OK".
    pub allowed: Signal<bool>,
}

impl Confirmation {
    pub fn new(
        cx: &mut Cx,
        confirm_label: &'static str,
        secondary_label: Option<&'static str>,
    ) -> Self {
        Self {
            title: cx.signal(String::new()),
            message: cx.signal(String::new()),
            confirm_label: cx.signal(confirm_label),
            secondary_label: cx.signal(secondary_label),
            allowed: cx.signal(true),
        }
    }
}

/// A modal confirmation with Cancel, an optional secondary action and a destructive confirm.
pub fn confirm_dialog(
    open: Signal<bool>,
    confirmation: Confirmation,
    on_choice: impl Fn(&mut Cx, ConfirmChoice) + Clone + 'static,
) -> impl View {
    dialog(open, move || {
        let on_choice = on_choice.clone();
        component(move |cx| {
            let theme = use_theme(cx);
            let allowed = confirmation.allowed;
            let choose = move |choice: ConfirmChoice| {
                let on_choice = on_choice.clone();
                move |cx: &mut Cx, _: &mut Event<ClickEvent>| {
                    open.set(cx, false);
                    if allowed.get(cx) {
                        on_choice(cx, choice);
                    }
                }
            };
            let blocked = bind(move |runtime| !allowed.get(runtime));
            let secondary_label = confirmation.secondary_label;
            let secondary = button(bind(move |runtime| {
                secondary_label.get(runtime).unwrap_or_default().to_owned()
            }))
            .variant(ButtonVariant::Secondary)
            .disabled(blocked)
            .hidden(bind(move |runtime| {
                secondary_label.get(runtime).is_none() || !allowed.get(runtime)
            }))
            .on_click(choose(ConfirmChoice::Secondary));
            let confirm_label = confirmation.confirm_label;
            let confirm = button(bind(move |runtime| confirm_label.get(runtime).to_owned()))
                .variant(ButtonVariant::Destructive)
                .disabled(bind(move |runtime| !allowed.get(runtime)))
                .hidden(bind(move |runtime| !allowed.get(runtime)))
                .on_click(choose(ConfirmChoice::Confirm));
            vstack((
                text(confirmation.title)
                    .text_size(TITLE_SIZE)
                    .font_semibold()
                    .role(Role::Heading),
                text(confirmation.message).text_color(bind(move |runtime| {
                    theme.with(runtime, |theme| theme.text.muted)
                })),
                hstack((
                    spacer(),
                    button(bind(move |runtime| {
                        if allowed.get(runtime) { "Cancel" } else { "OK" }.to_owned()
                    }))
                    .variant(ButtonVariant::Secondary)
                    .on_click(move |cx, _| open.set(cx, false)),
                    secondary,
                    confirm,
                ))
                .gap(spacing::COMFORTABLE),
            ))
            .gap(spacing::LOOSE)
            .p(DIALOG_PADDING)
            .w(DIALOG_WIDTH)
        })
    })
}
