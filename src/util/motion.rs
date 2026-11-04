use crownui::prelude::Curve;

/// A transition that lands on the next frame. Figures that change on every engine tick sit
/// under it, so a new reading repaints once instead of following an ancestor's spring or
/// theme fade for most of the second.
pub fn instant() -> Curve {
    Curve::linear(0)
}
