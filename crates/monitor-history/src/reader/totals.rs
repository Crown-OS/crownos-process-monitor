use monitor_core::Bytes;
use rusqlite::params;

use crate::error::HistoryResult;
use crate::range::Span;
use crate::reader::HistoryReader;
use crate::util::sql::{integer_to_bytes, real_to_bytes};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CounterTotals {
    /// Machine seconds (a fully busy machine adds one per second).
    pub cpu_busy_seconds: f64,
    pub disk_read: Bytes,
    pub disk_written: Bytes,
    pub network_received: Bytes,
    pub network_transmitted: Bytes,
}

impl CounterTotals {
    fn add(&mut self, other: &Self) {
        self.cpu_busy_seconds += other.cpu_busy_seconds;
        self.disk_read += other.disk_read;
        self.disk_written += other.disk_written;
        self.network_received += other.network_received;
        self.network_transmitted += other.network_transmitted;
    }
}

/// Range totals split into what was recorded and what was estimated from
/// since-boot counters while the app was closed.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Totals {
    pub recorded: CounterTotals,
    pub estimated: CounterTotals,
    pub recorded_seconds: f64,
    pub energy_joules: Option<f64>,
    pub includes_estimate: bool,
}

impl Totals {
    pub fn combined(&self) -> CounterTotals {
        let mut combined = self.recorded;
        combined.add(&self.estimated);
        combined
    }
}

impl HistoryReader {
    /// Sums over the span plus the overlapping share of estimated gap
    /// buckets (spread evenly, since their distribution is unknown).
    pub fn totals(&self, span: Span) -> HistoryResult<Totals> {
        let table = span.resolution().system_table();
        let sql = format!(
            "SELECT coalesce(sum(recorded_seconds), 0.0), coalesce(sum(cpu_busy_seconds), 0.0), \
             coalesce(sum(disk_read_bytes), 0), coalesce(sum(disk_written_bytes), 0), \
             coalesce(sum(network_received_bytes), 0), coalesce(sum(network_transmitted_bytes), 0), \
             sum(energy_joules) FROM {table} WHERE start_unix >= ?1 AND start_unix < ?2"
        );
        let mut totals = self.connection().prepare_cached(&sql)?.query_row(
            params![span.from_unix, span.to_unix],
            |row| {
                Ok(Totals {
                    recorded_seconds: row.get(0)?,
                    recorded: CounterTotals {
                        cpu_busy_seconds: row.get(1)?,
                        disk_read: integer_to_bytes(row.get(2)?),
                        disk_written: integer_to_bytes(row.get(3)?),
                        network_received: integer_to_bytes(row.get(4)?),
                        network_transmitted: integer_to_bytes(row.get(5)?),
                    },
                    energy_joules: row.get(6)?,
                    ..Totals::default()
                })
            },
        )?;
        for gap in self.gaps(span)?.iter().filter(|gap| gap.estimated) {
            let length = (gap.end_unix - gap.start_unix).max(1) as f64;
            let overlap =
                (gap.end_unix.min(span.to_unix) - gap.start_unix.max(span.from_unix)).max(0) as f64;
            let share = overlap / length;
            let scaled = |bytes: Option<Bytes>| {
                real_to_bytes(bytes.map_or(0.0, |bytes| bytes.get() as f64) * share)
            };
            totals.estimated.add(&CounterTotals {
                cpu_busy_seconds: gap.cpu_busy_seconds.unwrap_or(0.0) * share,
                disk_read: scaled(gap.disk_read),
                disk_written: scaled(gap.disk_written),
                network_received: scaled(gap.network_received),
                network_transmitted: scaled(gap.network_transmitted),
            });
            totals.includes_estimate |= share > 0.0;
        }
        Ok(totals)
    }
}
