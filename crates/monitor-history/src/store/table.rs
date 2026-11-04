use std::borrow::Cow;
use std::fmt::Write;

use crate::gauge::GaugeMetric;

const REAL: &str = "REAL NOT NULL";
const INTEGER: &str = "INTEGER NOT NULL";
const NULLABLE_REAL: &str = "REAL";
const START_KEY: (&str, &str) = ("start_unix", "INTEGER");

/// How a column combines when a second write hits the same bucket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Merge {
    Sum,
    Max,
    NullableSum,
    NullableMax,
    /// Average weighted by the row's `recorded_seconds`.
    WeightedAverage,
}

impl Merge {
    fn assignment(self, column: &str, output: &mut String) {
        let _ = match self {
            Self::Sum => write!(output, "{column} = {column} + excluded.{column}"),
            Self::Max => write!(output, "{column} = max({column}, excluded.{column})"),
            Self::NullableSum => write!(
                output,
                "{column} = coalesce({column} + excluded.{column}, {column}, excluded.{column})"
            ),
            Self::NullableMax => write!(
                output,
                "{column} = max(coalesce({column}, excluded.{column}), coalesce(excluded.{column}, {column}))"
            ),
            Self::WeightedAverage => write!(
                output,
                "{column} = coalesce(({column} * recorded_seconds + excluded.{column} * excluded.recorded_seconds) / (recorded_seconds + excluded.recorded_seconds), {column}, excluded.{column})"
            ),
        };
    }
}

pub struct Column {
    pub name: Cow<'static, str>,
    pub sql_type: &'static str,
    pub merge: Merge,
}

const fn column(name: &'static str, sql_type: &'static str, merge: Merge) -> Column {
    Column {
        name: Cow::Borrowed(name),
        sql_type,
        merge,
    }
}

/// A bucketed, mergeable table: one spec drives both its DDL and its upsert,
/// so the two cannot drift apart. Values bind in key-then-column order.
pub struct TableSpec {
    pub name: &'static str,
    pub keys: &'static [(&'static str, &'static str)],
    pub columns: Vec<Column>,
}

impl TableSpec {
    /// `IF NOT EXISTS`, so a migration step may safely run twice.
    pub fn create_sql(&self) -> String {
        let mut sql = format!("CREATE TABLE IF NOT EXISTS {} (", self.name);
        for (name, sql_type) in self.keys {
            let _ = write!(sql, "{name} {sql_type} NOT NULL, ");
        }
        for column in &self.columns {
            let _ = write!(sql, "{} {}, ", column.name, column.sql_type);
        }
        let key_names: Vec<_> = self.keys.iter().map(|(name, _)| *name).collect();
        let _ = write!(
            sql,
            "PRIMARY KEY ({})) STRICT, WITHOUT ROWID;",
            key_names.join(", ")
        );
        sql
    }

    pub fn upsert_sql(&self) -> String {
        let key_names: Vec<_> = self.keys.iter().map(|(name, _)| *name).collect();
        let column_names: Vec<_> = self.columns.iter().map(|c| c.name.as_ref()).collect();
        let placeholders: Vec<_> = (1..=key_names.len() + column_names.len())
            .map(|index| format!("?{index}"))
            .collect();
        let mut sql = format!(
            "INSERT INTO {} ({}, {}) VALUES ({}) ON CONFLICT ({}) DO UPDATE SET ",
            self.name,
            key_names.join(", "),
            column_names.join(", "),
            placeholders.join(", "),
            key_names.join(", "),
        );
        for (index, column) in self.columns.iter().enumerate() {
            if index > 0 {
                sql.push_str(", ");
            }
            column.merge.assignment(&column.name, &mut sql);
        }
        sql
    }
}

pub const SYSTEM_TABLES: [&str; 3] = ["system_minutes", "system_five_minutes", "system_hours"];
pub const APP_TABLES: [&str; 2] = ["app_hours", "app_days"];

pub fn system_table(name: &'static str) -> TableSpec {
    TableSpec {
        name,
        keys: &[START_KEY],
        columns: vec![
            column("recorded_seconds", REAL, Merge::Sum),
            column("cpu_busy_seconds", REAL, Merge::Sum),
            column("memory_byte_seconds", REAL, Merge::Sum),
            column("disk_read_bytes", INTEGER, Merge::Sum),
            column("disk_written_bytes", INTEGER, Merge::Sum),
            column("network_received_bytes", INTEGER, Merge::Sum),
            column("network_transmitted_bytes", INTEGER, Merge::Sum),
            column("energy_joules", NULLABLE_REAL, Merge::NullableSum),
        ],
    }
}

/// `recorded_seconds` must stay first: weighted averages refer to it.
pub fn readings_table() -> TableSpec {
    let mut columns = vec![
        column("recorded_seconds", REAL, Merge::Sum),
        column("pressure_level_peak", INTEGER, Merge::Max),
        column("throttled_seconds", REAL, Merge::Sum),
        column("on_ac_seconds", REAL, Merge::Sum),
    ];
    for metric in GaugeMetric::ALL {
        columns.push(Column {
            name: format!("{}_avg", metric.column()).into(),
            sql_type: NULLABLE_REAL,
            merge: Merge::WeightedAverage,
        });
        columns.push(Column {
            name: format!("{}_peak", metric.column()).into(),
            sql_type: NULLABLE_REAL,
            merge: Merge::NullableMax,
        });
    }
    TableSpec {
        name: "readings",
        keys: &[START_KEY],
        columns,
    }
}

pub fn app_table(name: &'static str) -> TableSpec {
    TableSpec {
        name,
        keys: &[START_KEY, ("app_id", "INTEGER")],
        columns: vec![
            column("seconds", REAL, Merge::Sum),
            column("cpu_seconds", REAL, Merge::Sum),
            column("memory_byte_seconds", REAL, Merge::Sum),
            column("peak_memory_bytes", INTEGER, Merge::Max),
            column("disk_read_bytes", INTEGER, Merge::Sum),
            column("disk_written_bytes", INTEGER, Merge::Sum),
            column("network_received_bytes", INTEGER, Merge::Sum),
            column("network_transmitted_bytes", INTEGER, Merge::Sum),
            column("gpu_seconds", REAL, Merge::Sum),
            column("energy_joules", NULLABLE_REAL, Merge::NullableSum),
        ],
    }
}

/// `gpu_five_minutes` was added in schema version 2.
pub const GPU_TABLES: [&str; 2] = ["gpu_five_minutes", "gpu_hours"];

pub fn gpu_table(name: &'static str) -> TableSpec {
    TableSpec {
        name,
        keys: &[START_KEY, ("gpu", "TEXT")],
        columns: vec![
            column("seconds", REAL, Merge::Sum),
            column("busy_seconds", REAL, Merge::Sum),
            column("vram_byte_seconds", REAL, Merge::Sum),
            column("energy_joules", NULLABLE_REAL, Merge::NullableSum),
            column("peak_celsius", NULLABLE_REAL, Merge::NullableMax),
        ],
    }
}

pub const GPU_ENGINE_TABLES: [&str; 2] = ["gpu_engine_five_minutes", "gpu_engine_hours"];

/// Busy time per GPU engine; added in schema version 2.
pub fn gpu_engine_table(name: &'static str) -> TableSpec {
    TableSpec {
        name,
        keys: &[START_KEY, ("gpu", "TEXT"), ("engine", "TEXT")],
        columns: vec![
            column("seconds", REAL, Merge::Sum),
            column("busy_seconds", REAL, Merge::Sum),
        ],
    }
}

pub fn device_table() -> TableSpec {
    TableSpec {
        name: "device_hours",
        keys: &[START_KEY, ("device", "TEXT")],
        columns: vec![
            column("read_bytes", INTEGER, Merge::Sum),
            column("written_bytes", INTEGER, Merge::Sum),
        ],
    }
}

pub fn interface_table() -> TableSpec {
    TableSpec {
        name: "interface_hours",
        keys: &[START_KEY, ("interface", "TEXT")],
        columns: vec![
            column("received_bytes", INTEGER, Merge::Sum),
            column("transmitted_bytes", INTEGER, Merge::Sum),
        ],
    }
}

/// The tables of schema version 1.
pub fn bucketed_tables() -> Vec<TableSpec> {
    let mut tables: Vec<_> = SYSTEM_TABLES.into_iter().map(system_table).collect();
    tables.push(readings_table());
    tables.extend(APP_TABLES.map(app_table));
    tables.extend([gpu_table("gpu_hours"), device_table(), interface_table()]);
    tables
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_matches_columns() {
        let spec = device_table();
        assert_eq!(
            spec.upsert_sql(),
            "INSERT INTO device_hours (start_unix, device, read_bytes, written_bytes) VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT (start_unix, device) DO UPDATE SET read_bytes = read_bytes + excluded.read_bytes, \
             written_bytes = written_bytes + excluded.written_bytes"
        );
        assert!(
            spec.create_sql()
                .starts_with("CREATE TABLE IF NOT EXISTS device_hours (")
        );
        assert!(
            spec.create_sql()
                .ends_with("PRIMARY KEY (start_unix, device)) STRICT, WITHOUT ROWID;")
        );
    }
}
