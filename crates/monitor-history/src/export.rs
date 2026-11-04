use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use rusqlite::types::ValueRef;
use rusqlite::{Connection, params};

use crate::error::{HistoryError, HistoryResult};
use crate::range::Span;
use crate::reader::HistoryReader;
use crate::util::csv::CsvWriter;
use crate::util::paths::download_directory;

const APPS_BY_DAY: &str = "SELECT strftime('%Y-%m-%d', u.start_unix, 'unixepoch') AS day_utc, \
    a.app_key, a.name, a.kind, u.seconds, u.cpu_seconds, \
    u.memory_byte_seconds / nullif(u.seconds, 0) AS memory_bytes_avg, u.peak_memory_bytes, \
    u.disk_read_bytes, u.disk_written_bytes, u.network_received_bytes, \
    u.network_transmitted_bytes, u.gpu_seconds, u.energy_joules \
    FROM app_days u JOIN apps a ON a.app_id = u.app_id \
    WHERE u.start_unix > ?1 - 86400 AND u.start_unix < ?2 ORDER BY u.start_unix, u.cpu_seconds DESC";

const SYSTEM_BY_HOUR: &str = "SELECT strftime('%Y-%m-%dT%H:%M:%SZ', start_unix, 'unixepoch') AS hour_utc, \
    recorded_seconds, 100.0 * cpu_busy_seconds / nullif(recorded_seconds, 0) AS cpu_percent_avg, \
    cpu_busy_seconds, memory_byte_seconds / nullif(recorded_seconds, 0) AS memory_bytes_avg, \
    disk_read_bytes, disk_written_bytes, network_received_bytes, network_transmitted_bytes, \
    energy_joules FROM system_hours \
    WHERE start_unix > ?1 - 3600 AND start_unix < ?2 ORDER BY start_unix";

const READINGS: &str = "SELECT strftime('%Y-%m-%dT%H:%M:%SZ', start_unix, 'unixepoch') AS start_utc, * \
    FROM readings WHERE start_unix > ?1 - 300 AND start_unix < ?2 ORDER BY start_unix";

const EXPORTS: [(&str, &str); 3] = [
    ("apps-by-day.csv", APPS_BY_DAY),
    ("system-by-hour.csv", SYSTEM_BY_HOUR),
    ("readings-every-5-minutes.csv", READINGS),
];

fn write_query_csv(
    connection: &Connection,
    sql: &str,
    span: Span,
    path: &Path,
) -> HistoryResult<()> {
    let mut statement = connection.prepare(sql)?;
    let mut csv = CsvWriter::new(BufWriter::new(File::create(path)?));
    for name in statement.column_names() {
        csv.text(name)?;
    }
    csv.end_record()?;
    let column_count = statement.column_count();
    let mut rows = statement.query(params![span.from_unix, span.to_unix])?;
    while let Some(row) = rows.next()? {
        for index in 0..column_count {
            match row.get_ref(index)? {
                ValueRef::Null | ValueRef::Blob(_) => csv.empty()?,
                ValueRef::Integer(value) => csv.number(value)?,
                ValueRef::Real(value) => csv.number(format_args!("{value:.3}"))?,
                ValueRef::Text(text) => csv.text(&String::from_utf8_lossy(text))?,
            }
        }
        csv.end_record()?;
    }
    csv.finish()?;
    Ok(())
}

impl HistoryReader {
    /// Writes apps by day, system by hour and five-minute readings into a
    /// new timestamped folder below `directory` (default: the XDG download
    /// directory). Returns the written files.
    pub fn export_csv(&self, directory: Option<&Path>, span: Span) -> HistoryResult<Vec<PathBuf>> {
        let parent = match directory {
            Some(directory) => directory.to_path_buf(),
            None => download_directory().ok_or(HistoryError::NoHomeDirectory)?,
        };
        let stamp: String = self.connection().query_row(
            "SELECT strftime('%Y%m%d-%H%M%S', ?1, 'unixepoch')",
            [span.to_unix],
            |row| row.get(0),
        )?;
        let folder = parent.join(format!("crownos-history-{stamp}"));
        std::fs::create_dir_all(&folder)?;
        EXPORTS
            .iter()
            .map(|(file, sql)| {
                let path = folder.join(file);
                write_query_csv(self.connection(), sql, span, &path)?;
                Ok(path)
            })
            .collect()
    }
}
