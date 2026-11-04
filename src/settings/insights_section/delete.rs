use crownui::kit::{ButtonOptions, ButtonVariant, button, use_theme};
use crownui::prelude::{Cx, Listeners, Styled, View, bind, hstack, show, spacer, text, vstack};
use monitor_engine::Command;

use crate::app::EngineLink;

const ROW_GAP: f32 = 8.0;
const CAPTION_SIZE: f32 = 12.0;

/// "Delete History…", confirmed in place before the engine truncates every table.
pub fn delete_history(cx: &mut Cx) -> impl View + use<> {
    let theme = use_theme(cx);
    let link = EngineLink::find(cx);
    let confirming = cx.signal(false);
    let deleted = cx.signal(false);
    let muted = move || bind(move |runtime| theme.with(runtime, |theme| theme.text.muted));
    show(
        confirming,
        move || {
            let link = link.clone();
            vstack((
                text("Delete every recorded reading, app hour and past alert? This cannot be undone.")
                    .text_size(CAPTION_SIZE),
                hstack((
                    spacer(),
                    button("Cancel")
                        .variant(ButtonVariant::Secondary)
                        .on_click(move |cx, _| confirming.set(cx, false)),
                    button("Delete")
                        .variant(ButtonVariant::Destructive)
                        .on_click(move |cx, _| {
                            if let Some(link) = &link {
                                link.send(Command::DeleteHistory);
                            }
                            deleted.set(cx, true);
                            confirming.set(cx, false);
                        }),
                ))
                .gap(ROW_GAP),
            ))
            .gap(ROW_GAP)
        },
        move || {
            hstack((
                text(bind(move |runtime| {
                    if deleted.get(runtime) {
                        "History deleted".to_owned()
                    } else {
                        "History is kept only while the monitor runs".to_owned()
                    }
                }))
                .text_size(CAPTION_SIZE)
                .text_color(muted()),
                spacer(),
                button("Delete History…")
                    .variant(ButtonVariant::Destructive)
                    .on_click(move |cx, _| confirming.set(cx, true)),
            ))
            .items_center()
        },
    )
    .py(ROW_GAP)
}
