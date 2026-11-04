use std::time::Duration;

use crownui::prelude::{Cx, Signal};
use monitor_core::util::time::{DAY_SECONDS, unix_now};
use monitor_engine::history::Span;
use monitor_engine::{EngineControl, utc_offset_seconds};

use crate::app::live::LiveSnapshot;

const DAYS_SHOWN: u16 = 7;
const REFRESH: Duration = Duration::from_secs(10 * 60);
/// 1970-01-01 was a Thursday: Monday-based weekday 3.
const EPOCH_WEEKDAY: i64 = 3;
const WEEK_DAYS: i64 = 7;

struct DailyNetwork {
    bytes: Vec<f32>,
    first_weekday: usize,
}

/// Fills the Overview's per-day network bars from history, off the UI
/// thread, now and every ten minutes.
pub fn refresh(cx: &mut Cx, live: Signal<LiveSnapshot>, engine: EngineControl) {
    let query = engine.clone();
    let days = async move {
        tokio::task::spawn_blocking(move || daily_network(&query))
            .await
            .ok()
            .flatten()
    };
    cx.spawn(days, move |cx, days| {
        if let Some(days) = days {
            live.update(cx, |snapshot| {
                snapshot.network.daily_bytes = days.bytes;
                snapshot.network.daily_first_weekday = days.first_weekday;
            });
        }
        cx.spawn(crate::util::delay::after(REFRESH), move |cx, ()| {
            refresh(cx, live, engine);
        });
    });
}

fn daily_network(engine: &EngineControl) -> Option<DailyNetwork> {
    let reader = engine.history_reader().ok()?;
    let offset = utc_offset_seconds();
    let now = unix_now();
    let today = Span::today(now, offset);
    let bytes = (0..i64::from(DAYS_SHOWN))
        .rev()
        .map(|days_ago| {
            let from = today.from_unix - days_ago * DAY_SECONDS;
            let to = if days_ago == 0 {
                now
            } else {
                from + DAY_SECONDS
            };
            reader.totals(Span::new(from, to)).map_or(0.0, |totals| {
                let combined = totals.combined();
                (combined.network_received.get() + combined.network_transmitted.get()) as f32
            })
        })
        .collect();
    let first_day = (today.from_unix + i64::from(offset)) / DAY_SECONDS - i64::from(DAYS_SHOWN - 1);
    Some(DailyNetwork {
        bytes,
        first_weekday: (first_day + EPOCH_WEEKDAY).rem_euclid(WEEK_DAYS) as usize,
    })
}
