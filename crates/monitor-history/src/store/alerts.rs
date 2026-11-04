use std::sync::Arc;

use monitor_core::AppKey;
use rusqlite::{Connection, Row, params};

use crate::error::HistoryResult;
use crate::range::Span;
use crate::store::HistoryStore;

const MACHINE_APP_KEY: &str = "";
const UPSERT_ALERT: &str = "INSERT INTO alerts (start_unix, kind, app_key, app_name, end_unix, peak, \
    limit_value, count, quieted, pinned) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
    ON CONFLICT (start_unix, kind, app_key) DO UPDATE SET app_name = excluded.app_name, \
    end_unix = excluded.end_unix, peak = excluded.peak, limit_value = excluded.limit_value, \
    count = excluded.count, quieted = excluded.quieted, pinned = excluded.pinned";
const SELECT_ALERTS: &str = "SELECT start_unix, kind, app_key, app_name, end_unix, peak, limit_value, \
    count, quieted, pinned FROM alerts WHERE end_unix >= ?1 AND start_unix < ?2 ORDER BY start_unix";

/// One Worth a Look row as stored. It uses primitive fields so this crate
/// does not depend on `monitor-alerts`; the engine converts `AlertRecord`
/// to and from it (`kind` is `AlertKind::code()`). The row's state is its
/// `end_unix` (last occurrence), `count`, `quieted` and `pinned`.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredAlert {
    pub kind: String,
    /// `None` for machine-wide alerts.
    pub app_key: Option<AppKey>,
    pub app_name: Option<Arc<str>>,
    pub start_unix: i64,
    pub end_unix: i64,
    pub peak: f64,
    pub limit: f64,
    pub count: u32,
    pub quieted: bool,
    pub pinned: bool,
}

fn alert_from_row(row: &Row<'_>) -> rusqlite::Result<StoredAlert> {
    let app_key: String = row.get(2)?;
    let app_name: String = row.get(3)?;
    Ok(StoredAlert {
        start_unix: row.get(0)?,
        kind: row.get(1)?,
        app_key: AppKey::parse(&app_key),
        app_name: (!app_name.is_empty()).then(|| app_name.into()),
        end_unix: row.get(4)?,
        peak: row.get(5)?,
        limit: row.get(6)?,
        count: row.get(7)?,
        quieted: row.get(8)?,
        pinned: row.get(9)?,
    })
}

/// Alerts whose lifetime overlaps `span`, oldest first.
pub(crate) fn load_alerts(connection: &Connection, span: Span) -> HistoryResult<Vec<StoredAlert>> {
    let mut statement = connection.prepare_cached(SELECT_ALERTS)?;
    let alerts = statement
        .query_map(params![span.from_unix, span.to_unix], alert_from_row)?
        .collect::<Result<_, _>>()?;
    Ok(alerts)
}

impl HistoryStore {
    /// Inserts or updates the row keyed by `(start_unix, kind, app_key)`.
    pub fn save_alert(&mut self, alert: &StoredAlert) -> HistoryResult<()> {
        self.connection
            .prepare_cached(UPSERT_ALERT)?
            .execute(params![
                alert.start_unix,
                alert.kind,
                alert
                    .app_key
                    .as_ref()
                    .map_or(MACHINE_APP_KEY, AppKey::as_str),
                alert.app_name.as_deref().unwrap_or_default(),
                alert.end_unix,
                alert.peak,
                alert.limit,
                alert.count,
                alert.quieted,
                alert.pinned,
            ])?;
        Ok(())
    }
}
