use crownui::prelude::{Cx, Runtime, Signal, View};
use monitor_engine::history::{AppMetric, AppUsage, HistoryReader, HistoryResult, TimeRange};

use super::query::{HistoryData, QueryClock, history_query};

/// The top apps by one metric over a range (recorded time only).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TopAppsRequest {
    pub range: TimeRange,
    pub metric: AppMetric,
    pub limit: usize,
}

fn fetch_top_apps(
    reader: &HistoryReader,
    request: &TopAppsRequest,
    clock: QueryClock,
) -> HistoryResult<Vec<AppUsage>> {
    reader.top_apps(
        request.range.span(clock.now_unix),
        request.metric,
        request.limit,
    )
}

/// Per-app history for a page's History mode: its table and its "Top App" callout.
pub fn history_top_apps<R>(
    cx: &mut Cx,
    request: R,
) -> (Signal<HistoryData<Vec<AppUsage>>>, impl View + use<R>)
where
    R: FnMut(&mut Runtime) -> Option<TopAppsRequest> + 'static,
{
    history_query(cx, request, fetch_top_apps)
}
