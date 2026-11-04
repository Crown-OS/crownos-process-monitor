use crownui::prelude::Runtime;

/// Lends one series (oldest sample first, NaN for a gap) to the sink it is called with.
pub trait SeriesSource: Fn(&mut Runtime, &mut dyn FnMut(&[f32])) + Clone + 'static {}

impl<T: Fn(&mut Runtime, &mut dyn FnMut(&[f32])) + Clone + 'static> SeriesSource for T {}

/// Lends several series at once, in a fixed order.
pub trait MultiSeriesSource: Fn(&mut Runtime, &mut dyn FnMut(&[&[f32]])) + Clone + 'static {}

impl<T: Fn(&mut Runtime, &mut dyn FnMut(&[&[f32]])) + Clone + 'static> MultiSeriesSource for T {}

/// Lends two series at once, for charts mirrored around an axis.
pub trait PairSource: Fn(&mut Runtime, &mut dyn FnMut(&[f32], &[f32])) + Clone + 'static {}

impl<T: Fn(&mut Runtime, &mut dyn FnMut(&[f32], &[f32])) + Clone + 'static> PairSource for T {}
