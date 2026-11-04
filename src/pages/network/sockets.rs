use std::fmt::Write as _;

use crownui::kit::{ButtonOptions, ButtonSize, ButtonVariant, StatusOptions, Tone, button, status};
use crownui::prelude::{
    AnyView, Cx, IntoProp, Listeners, Role, Semantic, Signal, Styled, View, bind, hstack, text,
    vstack,
};
use monitor_core::{Pid, ProcessKey};
use monitor_engine::Snapshot;
use monitor_engine::io::{BindScope, Connection, ListeningPort};

use super::socket_list::{ROW_HEIGHT, SocketSource, socket_list};
use crate::app::AppState;
use crate::inspector::{protocol_name, scope_label, tcp_state_name};
use crate::util::spacing;

const CELL_SIZE: f32 = 12.0;
const PORT_WIDTH: f32 = 90.0;
const ADDRESS_WIDTH: f32 = 150.0;
const REACH_WIDTH: f32 = 200.0;
const PROTOCOL_WIDTH: f32 = 40.0;
const ENDPOINT_WIDTH: f32 = 170.0;
const STATE_WIDTH: f32 = 90.0;
const QUEUE_WIDTH: f32 = 70.0;
/// The last column takes the remaining width.
const FILL_WIDTH: f32 = 0.0;

pub const fn scope_tone(scope: BindScope) -> Tone {
    match scope {
        BindScope::Loopback => Tone::Success,
        BindScope::AllInterfaces => Tone::Warning,
        BindScope::Specific => Tone::Neutral,
    }
}

/// The owner's key and name, from the process table of the same tick.
fn owner(snapshot: &Snapshot, pid: Option<Pid>) -> Option<(ProcessKey, String)> {
    let table = &snapshot.processes;
    let row = table.processes.get(table.index_of(pid?)?)?;
    Some((row.key, format!("{} ({})", row.name, row.key.pid.get())))
}

fn owner_cell(owner: Option<(ProcessKey, String)>, inspect: Signal<Option<ProcessKey>>) -> AnyView {
    match owner {
        Some((key, name)) => button(name)
            .variant(ButtonVariant::Ghost)
            .control_size(ButtonSize::Small)
            .on_click(move |cx, _| inspect.set(cx, Some(key)))
            .boxed(),
        None => text("Unknown owner").text_size(CELL_SIZE).boxed(),
    }
}

fn row_label<T: 'static>(
    source: SocketSource<T>,
    index: usize,
    write: fn(&T, &mut String),
) -> impl IntoProp<String> {
    bind(move |runtime| {
        source.read(runtime, index, String::new(), |_, socket| {
            let mut label = String::new();
            write(socket, &mut label);
            label
        })
    })
}

fn write_port(port: &ListeningPort, out: &mut String) {
    let _ = write!(
        out,
        "{} {}",
        protocol_name(port.protocol),
        port.address.port()
    );
}

fn write_endpoints(connection: &Connection, out: &mut String) {
    let _ = write!(out, "{} → {}", connection.local, connection.remote);
}

fn listening_row(
    source: SocketSource<ListeningPort>,
    index: usize,
    inspect: Signal<Option<ProcessKey>>,
) -> AnyView {
    hstack((
        source.cell(index, PORT_WIDTH, write_port),
        source.cell(index, ADDRESS_WIDTH, |port, out| {
            let _ = write!(out, "{}", port.address.ip());
        }),
        source.keyed(
            index,
            |_, port| port.scope,
            |scope| {
                hstack(status(scope_label(scope)).tone(scope_tone(scope)))
                    .w(REACH_WIDTH)
                    .shrink(0.0)
                    .boxed()
            },
        ),
        source.keyed(
            index,
            |snapshot, port| owner(snapshot, port.owner),
            move |owner| owner_cell(owner, inspect),
        ),
    ))
    .gap(spacing::COMFORTABLE)
    .items_center()
    .h(ROW_HEIGHT)
    .w_full()
    .role(Role::ListItem)
    .label(row_label(source, index, write_port))
    .boxed()
}

fn connection_row(
    source: SocketSource<Connection>,
    index: usize,
    inspect: Signal<Option<ProcessKey>>,
) -> AnyView {
    hstack((
        source.cell(index, PROTOCOL_WIDTH, |connection, out| {
            out.push_str(protocol_name(connection.protocol));
        }),
        source.cell(index, ENDPOINT_WIDTH, |connection, out| {
            let _ = write!(out, "{}", connection.local);
        }),
        source.cell(index, ENDPOINT_WIDTH, |connection, out| {
            let _ = write!(out, "{}", connection.remote);
        }),
        source.cell(index, STATE_WIDTH, |connection, out| {
            out.push_str(tcp_state_name(connection.state));
        }),
        source.cell(index, QUEUE_WIDTH, |connection, out| {
            let _ = write!(
                out,
                "{} / {}",
                connection.receive_queue, connection.send_queue
            );
        }),
        source.keyed(
            index,
            |snapshot, connection| owner(snapshot, connection.owner),
            move |owner| owner_cell(owner, inspect),
        ),
    ))
    .gap(spacing::COMFORTABLE)
    .items_center()
    .h(ROW_HEIGHT)
    .w_full()
    .role(Role::ListItem)
    .label(row_label(source, index, write_endpoints))
    .boxed()
}

fn header(titles: &[(&'static str, f32)]) -> impl View {
    let cells: Vec<AnyView> = titles
        .iter()
        .map(|(title, width)| {
            let cell = text(*title).text_size(CELL_SIZE).font_medium();
            if *width > FILL_WIDTH {
                cell.w(*width).shrink(0.0).boxed()
            } else {
                cell.boxed()
            }
        })
        .collect();
    hstack(cells).gap(spacing::COMFORTABLE).w_full()
}

pub fn listening_panel(
    cx: &mut Cx,
    state: AppState,
    inspect: Signal<Option<ProcessKey>>,
) -> impl View + use<> {
    let list = socket_list(
        cx,
        state.live,
        "Listening ports",
        |network| &network.listening,
        |port| (protocol_name(port.protocol), port.address, port.owner),
        move |source, index| listening_row(source, index, inspect),
    );
    vstack((
        header(&[
            ("Port", PORT_WIDTH),
            ("Address", ADDRESS_WIDTH),
            ("Reachable from", REACH_WIDTH),
            ("Owner", FILL_WIDTH),
        ]),
        list,
    ))
    .gap(spacing::SNUG)
    .w_full()
}

pub fn connections_panel(
    cx: &mut Cx,
    state: AppState,
    inspect: Signal<Option<ProcessKey>>,
) -> impl View + use<> {
    let list = socket_list(
        cx,
        state.live,
        "Connections",
        |network| &network.connections,
        |connection| {
            (
                protocol_name(connection.protocol),
                connection.local,
                connection.remote,
                connection.owner,
            )
        },
        move |source, index| connection_row(source, index, inspect),
    );
    vstack((
        header(&[
            ("Proto", PROTOCOL_WIDTH),
            ("Local", ENDPOINT_WIDTH),
            ("Remote", ENDPOINT_WIDTH),
            ("State", STATE_WIDTH),
            ("Queue in/out", QUEUE_WIDTH),
            ("Owner", FILL_WIDTH),
        ]),
        list,
    ))
    .gap(spacing::SNUG)
    .w_full()
}
