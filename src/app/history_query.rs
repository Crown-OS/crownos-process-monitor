//! History for every page's History mode, queried off the UI thread.
//!
//! Each hook takes a closure deriving the request from signals (return `None` to pause) and
//! returns the answer as a `Signal<HistoryData<_>>` plus a driver view to mount on the page.
//! A query runs on a blocking task with one cached reader, again whenever the request changes
//! and every minute while the driver stays mounted.

mod app_rankings;
mod devices;
mod query;
mod range;
mod series;
#[cfg(test)]
mod test_history;
mod totals;

pub use app_rankings::{TopAppsRequest, history_top_apps};
pub use devices::{InterfacePeriods, history_interface_totals};
pub use query::{Fetch, HistoryData, QueryClock, REFRESH_INTERVAL, history_query};
pub use range::{picked_range, recorded_share_text, recording_since_text, time_range};
pub use series::{
    HistorySeries, SeriesRequest, fetch_series, history_series, pair_source, series_source,
};
pub use totals::{UsageTotals, average_cpu_percent, history_totals};
