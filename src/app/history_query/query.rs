use std::time::Duration;

use crownui::prelude::{Cx, Runtime, Signal, Styled, View, keyed, vstack};
use monitor_core::util::time::unix_now;
use monitor_engine::history::{HistoryReader, HistoryResult};
use monitor_engine::utc_offset_seconds;

use crate::app::EngineLink;

/// How often a mounted query runs again.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(60);

/// A query's latest answer. The previous answer stays `Ready` while a newer one runs.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum HistoryData<T> {
    #[default]
    Loading,
    Ready(T),
    /// No engine, no database or a failed query.
    Unavailable,
}

impl<T> HistoryData<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Self::Ready(value) => Some(value),
            Self::Loading | Self::Unavailable => None,
        }
    }
}

/// When a query runs, in the user's local time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryClock {
    pub now_unix: i64,
    pub utc_offset_seconds: i32,
}

impl QueryClock {
    pub fn now() -> Self {
        Self {
            now_unix: unix_now(),
            utc_offset_seconds: utc_offset_seconds(),
        }
    }
}

/// Runs off the UI thread with the cached reader; plain functions keep every query `Send`.
pub type Fetch<K, T> = fn(&HistoryReader, &K, QueryClock) -> HistoryResult<T>;

/// Runs `fetch` for the request `request` derives, on a blocking task, whenever the request
/// changes and every [`REFRESH_INTERVAL`] after that; `None` pauses it. Mount the returned
/// driver anywhere on the page (it takes no space): its lifetime is the query's, so a page
/// that leaves the screen stops querying.
pub fn history_query<K, T, R>(
    cx: &mut Cx,
    request: R,
    fetch: Fetch<K, T>,
) -> (Signal<HistoryData<T>>, impl View + use<K, T, R>)
where
    K: Clone + PartialEq + Send + 'static,
    T: PartialEq + Send + 'static,
    R: FnMut(&mut Runtime) -> Option<K> + 'static,
{
    history_query_every(cx, request, fetch, REFRESH_INTERVAL)
}

/// [`history_query`] refreshed every `interval` instead of every minute.
fn history_query_every<K, T, R>(
    cx: &mut Cx,
    request: R,
    fetch: Fetch<K, T>,
    interval: Duration,
) -> (Signal<HistoryData<T>>, impl View + use<K, T, R>)
where
    K: Clone + PartialEq + Send + 'static,
    T: PartialEq + Send + 'static,
    R: FnMut(&mut Runtime) -> Option<K> + 'static,
{
    let data = cx.signal(HistoryData::Loading);
    let link = EngineLink::find(cx);
    let driver = keyed(request, move |cx, request: Option<K>| {
        match (&link, request) {
            (Some(link), Some(request)) => {
                let job = Job {
                    link: link.clone(),
                    fetch,
                    data,
                    interval,
                };
                run(cx, job, request);
            }
            (None, Some(_)) => publish(cx, data, HistoryData::Unavailable),
            (_, None) => {}
        }
        vstack(()).display_none()
    })
    .display_none();
    (data, driver)
}

/// What one mounted query keeps between runs.
struct Job<K, T> {
    link: EngineLink,
    fetch: Fetch<K, T>,
    data: Signal<HistoryData<T>>,
    interval: Duration,
}

fn run<K, T>(cx: &mut Cx, job: Job<K, T>, request: K)
where
    K: Clone + Send + 'static,
    T: PartialEq + Send + 'static,
{
    let (link, fetch, job_request) = (job.link.clone(), job.fetch, request.clone());
    let work = async move {
        tokio::task::spawn_blocking(move || {
            link.read(|reader| fetch(reader, &job_request, QueryClock::now()))
        })
        .await
        .ok()
        .and_then(Result::ok)
    };
    cx.spawn(work, move |cx, answer| {
        let answer = answer.map_or(HistoryData::Unavailable, HistoryData::Ready);
        publish(cx, job.data, answer);
        cx.spawn(crate::util::delay::after(job.interval), move |cx, ()| {
            run(cx, job, request);
        });
    });
}

/// Writes only a different answer, so charts repaint only when the data changed.
fn publish<T: PartialEq + 'static>(
    cx: &mut Cx,
    data: Signal<HistoryData<T>>,
    answer: HistoryData<T>,
) {
    if data.with_untracked(cx, |current| *current != answer) {
        data.set(cx, answer);
    }
}
