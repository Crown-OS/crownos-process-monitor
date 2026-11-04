use crownui::prelude::{Cx, Runtime, Signal, View};
use monitor_engine::history::{HistoryReader, HistoryResult, Span, Totals};

use super::query::{HistoryData, QueryClock, history_query};

const PERCENT: f32 = 100.0;

/// Counter totals since local midnight, over the last 7 and the last 30 local days.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct UsageTotals {
    pub today: Totals,
    pub last_7_days: Totals,
    pub last_30_days: Totals,
}

/// Mean CPU over the recorded part of `totals`, percent of the machine; NaN when nothing
/// was recorded.
pub fn average_cpu_percent(totals: &Totals) -> f32 {
    if totals.recorded_seconds <= 0.0 {
        return f32::NAN;
    }
    (totals.recorded.cpu_busy_seconds / totals.recorded_seconds) as f32 * PERCENT
}

fn fetch_totals(reader: &HistoryReader, _: &(), clock: QueryClock) -> HistoryResult<UsageTotals> {
    let QueryClock {
        now_unix,
        utc_offset_seconds,
    } = clock;
    Ok(UsageTotals {
        today: reader.totals(Span::today(now_unix, utc_offset_seconds))?,
        last_7_days: reader.totals(Span::last_7_days(now_unix, utc_offset_seconds))?,
        last_30_days: reader.totals(Span::last_30_days(now_unix, utc_offset_seconds))?,
    })
}

/// Today / 7 d / 30 d totals, refreshed every minute.
pub fn history_totals(cx: &mut Cx) -> (Signal<HistoryData<UsageTotals>>, impl View + use<>) {
    history_query(cx, |_: &mut Runtime| Some(()), fetch_totals)
}

#[cfg(test)]
mod tests {
    use monitor_engine::history::HistoryReader;

    use super::*;
    use crate::app::history_query::test_history::{TestResult, seeded_history};

    #[test]
    fn totals_come_from_the_recorded_minutes() -> TestResult {
        let history = seeded_history("totals")?;
        let reader = HistoryReader::open(&history.database())?;
        let clock = QueryClock {
            now_unix: history.now_unix,
            utc_offset_seconds: 0,
        };
        let totals = fetch_totals(&reader, &(), clock)?;
        assert!(totals.last_30_days.recorded_seconds >= totals.today.recorded_seconds);
        assert!((average_cpu_percent(&totals.last_7_days) - 40.0).abs() < 1e-3);
        Ok(())
    }
}
