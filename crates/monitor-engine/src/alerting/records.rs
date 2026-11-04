use monitor_alerts::{AlertApp, AlertKind, AlertRecord};
use monitor_history::StoredAlert;

pub(crate) fn to_stored(record: &AlertRecord) -> StoredAlert {
    StoredAlert {
        kind: record.kind.code().to_owned(),
        app_key: record.app.as_ref().map(|app| app.key.clone()),
        app_name: record.app.as_ref().map(|app| app.name.clone()),
        start_unix: record.start_unix,
        end_unix: record.end_unix,
        peak: record.peak,
        limit: record.limit,
        count: record.count,
        quieted: record.quieted,
        pinned: record.pinned,
    }
}

/// `None` for kinds this version no longer knows.
pub(crate) fn from_stored(stored: StoredAlert) -> Option<AlertRecord> {
    let kind = AlertKind::from_code(&stored.kind)?;
    let app = stored.app_key.map(|key| AlertApp {
        name: stored.app_name.unwrap_or_else(|| key.id().into()),
        key,
    });
    Some(AlertRecord {
        kind,
        app,
        start_unix: stored.start_unix,
        end_unix: stored.end_unix,
        peak: stored.peak,
        limit: stored.limit,
        count: stored.count,
        quieted: stored.quieted,
        pinned: stored.pinned,
    })
}

#[cfg(test)]
mod tests {
    use monitor_alerts::AlertApp;
    use monitor_core::{AppKey, AppKind};

    use super::*;

    #[test]
    fn records_round_trip_through_history() {
        let record = AlertRecord {
            kind: AlertKind::MemoryGrowth,
            app: Some(AlertApp {
                key: AppKey::new(AppKind::Desktop, "firefox"),
                name: "Firefox".into(),
            }),
            start_unix: 100,
            end_unix: 160,
            peak: 2e9,
            limit: 1e9,
            count: 2,
            quieted: false,
            pinned: true,
        };
        assert_eq!(from_stored(to_stored(&record)), Some(record));
        let unknown = StoredAlert {
            kind: "retired_rule".into(),
            ..to_stored(&AlertRecord {
                kind: AlertKind::GpuBusy,
                app: None,
                start_unix: 0,
                end_unix: 0,
                peak: 0.0,
                limit: 0.0,
                count: 1,
                quieted: false,
                pinned: false,
            })
        };
        assert_eq!(from_stored(unknown), None);
    }
}
