use crownui::prelude::{
    AnyView, Cx, Runtime, Semantic, Signal, Store, Styled, View, bind, keyed, lazy_list, text,
    vstack,
};
use monitor_engine::Snapshot;
use monitor_engine::io::NetworkSnapshot;

use crate::app::live::LiveSnapshot;

pub const ROW_HEIGHT: f32 = 30.0;
const LIST_HEIGHT: f32 = 240.0;
const CELL_SIZE: f32 = 12.0;

/// The sockets of one of the tick's vectors in a stable order, whatever order the kernel
/// listed them in this time. Rows read the socket shown at their index through it, so they
/// refresh in place when sockets come and go.
pub struct SocketSource<T: 'static> {
    live: Signal<LiveSnapshot>,
    sockets: fn(&NetworkSnapshot) -> &[T],
    order: Store<Vec<usize>>,
    figures: Signal<u64>,
}

impl<T: 'static> Clone for SocketSource<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> Copy for SocketSource<T> {}

impl<T: 'static> SocketSource<T> {
    /// `read` of the socket shown at `index` and the tick it was listed in, or `fallback`
    /// past the end; re-run by the calling observer after every tick.
    pub fn read<R>(
        self,
        runtime: &mut Runtime,
        index: usize,
        fallback: R,
        read: impl FnOnce(&Snapshot, &T) -> R,
    ) -> R {
        self.figures.get(runtime);
        let Some(position) = self.order.with(runtime, |order| order.get(index).copied()) else {
            return fallback;
        };
        self.live.with_untracked(runtime, |snapshot| {
            match (self.sockets)(&snapshot.tick.network).get(position) {
                Some(socket) => read(&snapshot.tick, socket),
                None => fallback,
            }
        })
    }

    /// A text cell `width` wide of the socket shown at `index`.
    pub fn cell(self, index: usize, width: f32, write: fn(&T, &mut String)) -> AnyView {
        let content = bind(move |runtime| {
            self.read(runtime, index, String::new(), |_, socket| {
                let mut content = String::new();
                write(socket, &mut content);
                content
            })
        });
        text(content)
            .text_size(CELL_SIZE)
            .w(width)
            .shrink(0.0)
            .truncate()
            .boxed()
    }

    /// `build(key)` for the socket shown at `index`, built again only when its key changes.
    pub fn keyed<K: Clone + PartialEq + 'static>(
        self,
        index: usize,
        key: fn(&Snapshot, &T) -> K,
        mut build: impl FnMut(K) -> AnyView + 'static,
    ) -> AnyView {
        keyed(
            move |runtime| {
                self.read(runtime, index, None, |snapshot, socket| {
                    Some(key(snapshot, socket))
                })
            },
            move |_, key| match key {
                Some(key) => build(key),
                None => vstack(()).boxed(),
            },
        )
        .boxed()
    }
}

/// A virtual list over one of the tick's socket vectors, ordered by `key`. Rows are built once
/// per index and read the socket shown there through the [`SocketSource`] they get.
pub fn socket_list<T, K, R>(
    cx: &mut Cx,
    live: Signal<LiveSnapshot>,
    label: &'static str,
    sockets: fn(&NetworkSnapshot) -> &[T],
    key: fn(&T) -> K,
    row: R,
) -> impl View + use<T, K, R>
where
    T: 'static,
    K: Ord + 'static,
    R: Fn(SocketSource<T>, usize) -> AnyView + 'static,
{
    let source = SocketSource {
        live,
        sockets,
        order: cx.store(Vec::new()),
        figures: cx.signal(0_u64),
    };
    let count = cx.signal(0_usize);
    let rows_refresh_in_place = cx.signal(0_u64);
    let mut sorted: Vec<(K, usize)> = Vec::new();
    cx.effect(move |runtime| {
        live.with(runtime, |snapshot| {
            sorted.clear();
            sorted.extend(
                sockets(&snapshot.tick.network)
                    .iter()
                    .enumerate()
                    .map(|(position, socket)| (key(socket), position)),
            );
        });
        sorted.sort_unstable_by(|first, second| first.0.cmp(&second.0));
        source.order.update(runtime, |order| {
            order.clear();
            order.extend(sorted.iter().map(|(_, position)| *position));
        });
        let length = sorted.len();
        if count.with_untracked(runtime, |count| *count != length) {
            count.set(runtime, length);
        }
        source.figures.update(runtime, |figures| *figures += 1);
    });
    lazy_list(count, rows_refresh_in_place, ROW_HEIGHT, move |_, index| {
        row(source, index)
    })
    .h(LIST_HEIGHT)
    .w_full()
    .label(label)
}
