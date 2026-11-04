use monitor_alerts::{Alert, AlertKind, AlertUnit};

const BYTES_PER_GIGABYTE: f64 = 1e9;
const BYTES_PER_MEGABYTE: f64 = 1e6;

/// Summary and body of the popup for `alert`.
pub(super) fn describe(alert: &Alert) -> (String, String) {
    let subject = alert.app.as_ref().map_or("Your computer", |app| &app.name);
    let value = amount(alert.kind.unit(), alert.value);
    let limit = amount(alert.kind.unit(), alert.limit);
    let (summary, body) = match alert.kind {
        AlertKind::SustainedCpu => (
            format!("{subject} has been busy for a while"),
            format!("It is using {value} of the processor."),
        ),
        AlertKind::MemoryGrowth => (
            format!("{subject} keeps using more memory"),
            format!("It grew by {value}."),
        ),
        AlertKind::DiskBurst => (
            format!("{subject} is writing or reading a lot"),
            format!("Disk activity reached {value}."),
        ),
        AlertKind::NetworkBurst => (
            format!("{subject} is moving a lot of data"),
            format!("Network traffic reached {value}."),
        ),
        AlertKind::MachineHot => (
            format!("{subject} is running hot"),
            format!("The hottest sensor reads {value}."),
        ),
        AlertKind::GpuBusy => (
            "The graphics card has been busy for a while".to_owned(),
            format!("It is at {value}."),
        ),
        AlertKind::MemoryLimit | AlertKind::CpuLimit => (
            format!("{subject} went over your limit"),
            format!("It reached {value}; your limit is {limit}."),
        ),
    };
    (summary, body)
}

fn amount(unit: AlertUnit, value: f64) -> String {
    match unit {
        AlertUnit::Percent => format!("{value:.0}%"),
        AlertUnit::Bytes => format!("{:.1} GB", value / BYTES_PER_GIGABYTE),
        AlertUnit::BytesPerSecond => format!("{:.0} MB/s", value / BYTES_PER_MEGABYTE),
        AlertUnit::Celsius => format!("{value:.0} °C"),
    }
}

#[cfg(test)]
mod tests {
    use monitor_alerts::AlertApp;
    use monitor_core::{AppKey, AppKind};

    use super::*;

    #[test]
    fn names_the_app_and_the_figure() {
        let alert = Alert {
            kind: AlertKind::CpuLimit,
            app: Some(AlertApp {
                key: AppKey::new(AppKind::Desktop, "firefox"),
                name: "Firefox".into(),
            }),
            at_unix: 0,
            value: 85.0,
            limit: 80.0,
        };
        let (summary, body) = describe(&alert);
        assert_eq!(summary, "Firefox went over your limit");
        assert_eq!(body, "It reached 85%; your limit is 80%.");
    }
}
