//! Charts drawn on one canvas each, repainted once per data change.

mod area;
mod axis;
mod bars;
mod decimate;
mod hover;
mod lines;
mod mirrored;
mod paint;
mod scale;
mod source;
mod stacked;
mod trace;

pub use area::{AreaChart, area_chart};
pub use axis::AxisLabels;
pub use bars::{BarChart, bar_chart};
pub use lines::{Legend, LineChart, Series, line_chart};
pub use mirrored::{MirroredChart, mirrored_chart};
pub use scale::YRange;
pub use source::{MultiSeriesSource, PairSource, SeriesSource};
pub use stacked::{StackedChart, stacked_chart};

/// Formats the value under the pointer for a chart's readout.
pub type Readout = Box<dyn Fn(&mut crownui::prelude::Runtime, f32) -> String>;
