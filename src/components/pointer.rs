use crownui::ext::{Kind, Listener, NodeId};
use crownui::prelude::{Cx, Element, Point, Signal, Size, View};

/// Where the pointer rests over a tracked element, in that element's own coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hover {
    pub position: Point,
    pub size: Size,
}

impl Hover {
    /// How far across the element the pointer is, in `0..=1`.
    pub fn fraction_x(self) -> f32 {
        if self.size.width <= 0.0 {
            return 0.0;
        }
        (self.position.x / self.size.width).clamp(0.0, 1.0)
    }
}

/// The kind behind [`pointer_tracker`].
pub struct PointerTrackerKind<V> {
    content: V,
    hover: Signal<Option<Hover>>,
}

impl<V: View> Kind for PointerTrackerKind<V> {
    const NAME: &'static str = "pointer_tracker";

    fn mount(self, cx: &mut Cx, node: NodeId) {
        let hover = self.hover;
        cx.set_children(node, self.content);
        cx.add_listener(
            node,
            Listener::PointerMove(Box::new(move |cx, event| {
                let Some(bounds) = cx
                    .tree()
                    .get(node)
                    .map(|element| element.geometry().window_rect)
                else {
                    return;
                };
                let position = Point::new(
                    event.position.x - bounds.origin.x,
                    event.position.y - bounds.origin.y,
                );
                hover.set(
                    cx,
                    Some(Hover {
                        position,
                        size: bounds.size,
                    }),
                );
            })),
        );
        cx.add_listener(
            node,
            Listener::PointerLeave(Box::new(move |cx, _| hover.set(cx, None))),
        );
    }
}

/// Reports the pointer's position over `content` into `hover`, and `None` once it leaves.
pub fn pointer_tracker<V: View>(
    hover: Signal<Option<Hover>>,
    content: V,
) -> Element<PointerTrackerKind<V>> {
    Element::new(PointerTrackerKind { content, hover })
}
