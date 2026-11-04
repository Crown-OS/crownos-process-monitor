//! History of every network interface.

use std::sync::Arc;

use crownui::prelude::{Cx, Runtime, Signal, View};
use monitor_engine::history::{HistoryReader, HistoryResult, Span};

use super::query::{HistoryData, QueryClock, history_query};

/// Bytes received and sent by one interface today, in 7 and in 30 days.
#[derive(Debug, Clone, PartialEq)]
pub struct InterfacePeriods {
    pub interface: Arc<str>,
    /// `[received, transmitted]` per period.
    pub periods: [[u64; 2]; 3],
}

fn fetch_interface_totals(
    reader: &HistoryReader,
    _: &(),
    clock: QueryClock,
) -> HistoryResult<Vec<InterfacePeriods>> {
    let QueryClock {
        now_unix,
        utc_offset_seconds,
    } = clock;
    let spans = [
        Span::today(now_unix, utc_offset_seconds),
        Span::last_7_days(now_unix, utc_offset_seconds),
        Span::last_30_days(now_unix, utc_offset_seconds),
    ];
    let mut periods: Vec<InterfacePeriods> = Vec::new();
    for (index, span) in spans.into_iter().enumerate() {
        for totals in reader.interface_totals(span)? {
            let position = periods
                .iter()
                .position(|known| known.interface == totals.interface)
                .unwrap_or_else(|| {
                    periods.push(InterfacePeriods {
                        interface: Arc::clone(&totals.interface),
                        periods: [[0; 2]; 3],
                    });
                    periods.len() - 1
                });
            periods[position].periods[index] = [totals.received.get(), totals.transmitted.get()];
        }
    }
    Ok(periods)
}

/// Per-interface totals for today, 7 and 30 days, refreshed every minute.
pub fn history_interface_totals(
    cx: &mut Cx,
) -> (
    Signal<HistoryData<Vec<InterfacePeriods>>>,
    impl View + use<>,
) {
    history_query(cx, |_: &mut Runtime| Some(()), fetch_interface_totals)
}
