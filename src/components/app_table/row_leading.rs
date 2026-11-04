use crownui::ext::Prop;
use crownui::kit::{StatusOptions, Theme, Tone, icon, icon_muted, icons, status};
use crownui::prelude::{
    AnyView, Color, Cx, IntoProp, Listeners, Memo, Role, Runtime, Semantic, Signal, Styled, View,
    bind, hstack, keyed, text, vstack,
};

use super::data::{RowMark, RowRef, TableRows};
use super::header::CELL_GAP;
use super::row_source::RowSource;
use super::state::TableState;

const TEXT_SIZE: f32 = 13.0;
const DETAIL_SIZE: f32 = 12.0;
const ICON_SIZE: f32 = 16.0;
const PIN_SIZE: f32 = 12.0;
const DISCLOSURE_SIZE: f32 = 18.0;
const PROCESS_INDENT: f32 = 26.0;

/// Whether a row is an app or one of its processes: the only thing that changes how a row
/// is built rather than what it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RowKind {
    App,
    Process,
}

impl RowKind {
    pub fn of(row: RowRef) -> Self {
        match row {
            RowRef::App(_) => Self::App,
            RowRef::Process(..) => Self::Process,
        }
    }
}

/// The current row's name, for its text and labels.
pub(super) fn row_name<S, D>(cx: &mut Cx, source: RowSource<S, D>) -> Memo<String>
where
    S: 'static,
    D: TableRows + ?Sized + 'static,
{
    cx.memo(move |runtime| {
        source.read(runtime, String::new(), |data, row| {
            data.name(row).to_owned()
        })
    })
}

fn spacer_box() -> AnyView {
    vstack(()).w(DISCLOSURE_SIZE).shrink(0.0).boxed()
}

/// The expand chevron of an app row with processes; an empty slot otherwise.
fn disclosure<S, D>(source: RowSource<S, D>, name: Memo<String>, state: TableState) -> impl View
where
    S: 'static,
    D: TableRows + ?Sized + 'static,
{
    keyed(
        move |runtime| {
            let (app, processes) = source.read(runtime, (0, 0), |data, row| {
                (data.row_id(row).app, data.process_count(row.app()))
            });
            (processes > 0).then(|| state.expanded.with(runtime, |keys| keys.contains(&app)))
        },
        move |_, open| {
            let Some(expanded) = open else {
                return spacer_box();
            };
            let (glyph, verb) = if expanded {
                (icons::CHEVRON_DOWN, "Collapse")
            } else {
                (icons::CHEVRON_RIGHT, "Expand")
            };
            vstack(icon_muted(glyph).size(ICON_SIZE - 2.0))
                .size(DISCLOSURE_SIZE)
                .shrink(0.0)
                .items_center()
                .justify_center()
                .role(Role::Button)
                .label(bind(move |runtime| format!("{verb} {}", name.get(runtime))))
                .on_click(move |cx, event| {
                    event.stop_propagation();
                    if let Some(id) = source.id(cx) {
                        state.toggle_expanded(cx, id.app);
                    }
                })
                .boxed()
        },
    )
}

fn mark_badge(mark: RowMark) -> AnyView {
    match mark {
        RowMark::None => vstack(()).boxed(),
        RowMark::New => status("New").tone(Tone::Success).boxed(),
        RowMark::Ended => status("Ended").tone(Tone::Neutral).boxed(),
    }
}

/// What only an app row carries: its icon, process count and pin.
struct AppExtras {
    icon: AnyView,
    count: AnyView,
    pin: AnyView,
}

fn app_extras<S, D>(source: RowSource<S, D>, state: TableState, theme: Signal<Theme>) -> AppExtras
where
    S: 'static,
    D: TableRows + ?Sized + 'static,
{
    let processes = move |runtime: &mut Runtime| {
        source.read(runtime, 0, |data, row| data.process_count(row.app()))
    };
    let pinned = move |runtime: &mut Runtime| {
        source
            .id(runtime)
            .is_some_and(|id| state.pinned.with(runtime, |keys| keys.contains(&id.app)))
    };
    AppExtras {
        icon: keyed(
            move |runtime| source.read(runtime, "", |data, row| data.icon(row.app())),
            |_, body| icon(body).size(ICON_SIZE),
        )
        .boxed(),
        count: text(bind(move |runtime| processes(runtime).to_string()))
            .text_size(DETAIL_SIZE)
            .text_color(muted(theme))
            .hidden(bind(move |runtime| processes(runtime) <= 1))
            .boxed(),
        pin: icon_muted(icons::PIN)
            .size(PIN_SIZE)
            .hidden(bind(move |runtime| !pinned(runtime)))
            .boxed(),
    }
}

fn muted(theme: Signal<Theme>) -> Prop<Color> {
    bind(move |runtime| theme.with(runtime, |theme| theme.text.muted)).into_prop()
}

/// Name, disclosure, icon, process count, detail, mark and pin of whatever row is at the
/// source's index.
pub(super) fn leading<S, D>(
    cx: &mut Cx,
    source: RowSource<S, D>,
    kind: RowKind,
    name: Memo<String>,
    state: TableState,
    theme: Signal<Theme>,
) -> impl View + use<S, D>
where
    S: 'static,
    D: TableRows + ?Sized + 'static,
{
    let detail = cx.memo(move |runtime| {
        source.read(runtime, String::new(), |data, row| {
            let mut detail = String::new();
            data.detail(row, &mut detail);
            detail
        })
    });
    let mark = move |runtime: &mut Runtime| {
        source.read(runtime, RowMark::None, |data, row| data.mark(row))
    };
    let is_app = kind == RowKind::App;
    let indent = (!is_app).then(|| vstack(()).w(PROCESS_INDENT));
    let disclosure = if is_app {
        disclosure(source, name, state).boxed()
    } else {
        spacer_box()
    };
    let (app_icon, count, pin) = is_app
        .then(|| app_extras(source, state, theme))
        .map_or((None, None, None), |extras| {
            (Some(extras.icon), Some(extras.count), Some(extras.pin))
        });
    hstack((
        indent,
        disclosure,
        app_icon,
        text(name)
            .text_size(TEXT_SIZE)
            .truncate()
            .flex_1()
            .min_w(0.0),
        count,
        text(detail)
            .text_size(DETAIL_SIZE)
            .text_color(muted(theme))
            .truncate()
            .shrink(1.0)
            .min_w(0.0)
            .hidden(bind(move |runtime| detail.with(runtime, String::is_empty))),
        keyed(mark, |_, mark| mark_badge(mark))
            .hidden(bind(move |runtime| mark(runtime) == RowMark::None)),
        pin,
    ))
    .gap(CELL_GAP)
    .items_center()
    .flex_1()
    .min_w(0.0)
}
