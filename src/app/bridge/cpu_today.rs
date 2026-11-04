use std::time::Duration;

use crownui::prelude::{Cx, Signal};
use monitor_core::util::time::unix_now;
use monitor_engine::history::Span;
use monitor_engine::{EngineControl, utc_offset_seconds};

use crate::app::live::LiveSnapshot;

const REFRESH: Duration = Duration::from_secs(5 * 60);
const PERCENT: f64 = 100.0;

/// Fills the CPU page's "average today" from history, off the UI thread, now and every five
/// minutes.
pub fn refresh(cx: &mut Cx, live: Signal<LiveSnapshot>, engine: EngineControl) {
    let query = engine.clone();
    let average = async move {
        tokio::task::spawn_blocking(move || average_today(&query))
            .await
            .ok()
            .flatten()
    };
    cx.spawn(average, move |cx, average| {
        if average.is_some() {
            live.update(cx, |snapshot| snapshot.cpu.average_today = average);
        }
        cx.spawn(crate::util::delay::after(REFRESH), move |cx, ()| {
            refresh(cx, live, engine);
        });
    });
}

fn average_today(engine: &EngineControl) -> Option<f32> {
    let reader = engine.history_reader().ok()?;
    let totals = reader
        .totals(Span::today(unix_now(), utc_offset_seconds()))
        .ok()?;
    (totals.recorded_seconds > 0.0)
        .then(|| (totals.recorded.cpu_busy_seconds / totals.recorded_seconds * PERCENT) as f32)
}
