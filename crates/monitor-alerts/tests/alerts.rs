use monitor_alerts::{
    Alert, AlertDetector, AlertInput, AlertKind, AppLimit, AppObservation, CustomLimit,
    PressureLevel, QuietScope, QuietUntil, StrainLimit, Thresholds, WorthALook,
};
use monitor_core::{AppKey, AppKind, Bytes, BytesPerSecond, Celsius, Percent};

const BASE_UNIX: i64 = 1_800_000_000 - 1_800_000_000 % 86_400;
const TICK_SECONDS: i64 = 5;
const GIB: u64 = 1 << 30;
const MIB: u64 = 1 << 20;

fn app(cpu: f32, memory: u64, disk: f64) -> AppObservation {
    AppObservation {
        key: AppKey::new(AppKind::Desktop, "org.example.Editor"),
        name: "Editor".into(),
        cpu_percent: Percent::new(cpu),
        memory: Bytes::new(memory),
        disk_bytes_per_second: BytesPerSecond::new(disk),
        network_bytes_per_second: BytesPerSecond::ZERO,
        gpu_percent: Percent::ZERO,
    }
}

fn input<'a>(wall: i64, apps: &'a [AppObservation]) -> AlertInput<'a> {
    AlertInput {
        wall_unix_seconds: wall,
        elapsed_seconds: TICK_SECONDS as f64,
        apps,
        cpu_percent: Percent::new(20.0),
        gpu_percent: Percent::ZERO,
        hottest_celsius: Some(Celsius::new(50.0)),
        pressure_level: PressureLevel::Normal,
        disk_bytes_per_second: BytesPerSecond::ZERO,
        network_bytes_per_second: BytesPerSecond::ZERO,
    }
}

/// Ticks through `minutes`, asking `apps_at(minute)` for each tick's apps.
fn run(
    detector: &mut AlertDetector,
    minutes: i64,
    mut apps_at: impl FnMut(i64) -> Vec<AppObservation>,
) -> Vec<Alert> {
    let mut fired = Vec::new();
    for wall in (BASE_UNIX..=BASE_UNIX + minutes * 60).step_by(TICK_SECONDS as usize) {
        let apps = apps_at((wall - BASE_UNIX) / 60);
        detector.observe(&input(wall, &apps), &mut fired);
    }
    fired
}

fn kinds(alerts: &[Alert]) -> Vec<(AlertKind, i64)> {
    alerts
        .iter()
        .map(|alert| (alert.kind, alert.at_unix - BASE_UNIX))
        .collect()
}

#[test]
fn sustained_cpu_fires_once_then_after_cooldown() {
    let mut detector = AlertDetector::default();
    let alerts = run(&mut detector, 30, |_| vec![app(80.0, GIB, 0.0)]);
    assert_eq!(kinds(&alerts), [(AlertKind::SustainedCpu, 600)]);
    assert_eq!(alerts[0].app.as_ref().map(|app| &*app.name), Some("Editor"));

    let mut detector = AlertDetector::default();
    let alerts = run(&mut detector, 75, |_| vec![app(80.0, GIB, 0.0)]);
    assert_eq!(
        kinds(&alerts),
        [
            (AlertKind::SustainedCpu, 600),
            (AlertKind::SustainedCpu, 4200)
        ]
    );
}

#[test]
fn short_spikes_do_not_fire() {
    let mut detector = AlertDetector::default();
    let alerts = run(&mut detector, 90, |minute| {
        let busy = minute % 10 < 5;
        vec![app(if busy { 100.0 } else { 10.0 }, GIB, 0.0)]
    });
    assert!(alerts.is_empty(), "{alerts:?}");
}

#[test]
fn quieting_suppresses_until_it_expires() {
    let key = AppKey::new(AppKind::Desktop, "org.example.Editor");
    let mut detector = AlertDetector::default();
    detector.quiet(
        QuietScope::App(key.clone()),
        QuietUntil::Time {
            unix_seconds: BASE_UNIX + 1200,
        },
    );
    let alerts = run(&mut detector, 30, |_| vec![app(80.0, GIB, 0.0)]);
    assert_eq!(kinds(&alerts), [(AlertKind::SustainedCpu, 1200)]);
    assert!(detector.quiet_rules().is_empty());

    let mut detector = AlertDetector::default();
    detector.quiet(
        QuietScope::Kind(AlertKind::SustainedCpu),
        QuietUntil::Forever,
    );
    assert!(run(&mut detector, 30, |_| vec![app(80.0, GIB, 0.0)]).is_empty());
    detector.unquiet(&QuietScope::Kind(AlertKind::SustainedCpu));
    assert_eq!(detector.quiet_rules().len(), 0);
}

#[test]
fn custom_memory_limit_fires_after_its_minutes() {
    let limit = CustomLimit {
        app: AppKey::new(AppKind::Desktop, "org.example.Editor"),
        limit: AppLimit::Memory(Bytes::new(8 * GIB)),
        minutes: 5,
    };
    let mut detector = AlertDetector::default();
    detector.set_custom_limits([limit.clone()]);
    let alerts = run(&mut detector, 20, |_| vec![app(1.0, 9 * GIB, 0.0)]);
    assert_eq!(kinds(&alerts), [(AlertKind::MemoryLimit, 300)]);
    assert_eq!(alerts[0].limit, (8 * GIB) as f64);

    let mut detector = AlertDetector::default();
    detector.set_custom_limits([limit]);
    assert!(run(&mut detector, 20, |_| vec![app(1.0, 7 * GIB, 0.0)]).is_empty());
}

#[test]
fn steady_memory_growth_fires_once() {
    let mut detector = AlertDetector::default();
    let alerts = run(&mut detector, 90, |minute| {
        let noise = if minute % 3 == 0 { 300 * MIB } else { 0 };
        vec![app(
            1.0,
            GIB + minute.unsigned_abs() * 30 * MIB + noise,
            0.0,
        )]
    });
    assert_eq!(kinds(&alerts), [(AlertKind::MemoryGrowth, 3600)]);
    assert!(alerts[0].value >= GIB as f64);

    let mut detector = AlertDetector::default();
    let flat = run(&mut detector, 90, |minute| {
        let noise = if minute % 3 == 0 { 2 * GIB } else { 0 };
        vec![app(1.0, GIB + noise, 0.0)]
    });
    assert!(flat.is_empty());
}

#[test]
fn disk_burst_far_above_baseline_fires() {
    let mut detector = AlertDetector::default();
    let alerts = run(&mut detector, 40, |minute| {
        let rate = if minute >= 30 { 200e6 } else { 1e6 };
        vec![app(1.0, GIB, rate)]
    });
    assert_eq!(kinds(&alerts), [(AlertKind::DiskBurst, 1860)]);
}

#[test]
fn machine_heat_and_strain() {
    let mut detector = AlertDetector::default();
    let mut fired = Vec::new();
    for wall in (BASE_UNIX..=BASE_UNIX + 600).step_by(TICK_SECONDS as usize) {
        let mut tick = input(wall, &[]);
        tick.hottest_celsius = Some(Celsius::new(95.0));
        tick.cpu_percent = Percent::new(97.0);
        detector.observe(&tick, &mut fired);
    }
    assert_eq!(kinds(&fired), [(AlertKind::MachineHot, 300)]);
    let strain = detector.strain();
    assert!(strain.cpu.is_strained() && strain.temperature.is_strained());
    assert!(!strain.memory.is_strained());
    assert_eq!(strain.cpu.seconds(BASE_UNIX + 600), 600);
    assert_eq!(
        strain.temperature.limit,
        StrainLimit::Celsius(Celsius::new(90.0))
    );
}

#[test]
fn tracked_apps_stay_bounded_and_idle_ones_are_evicted() {
    let many: Vec<_> = (0..400)
        .map(|index| AppObservation {
            key: AppKey::new(AppKind::Process, &index.to_string()),
            ..app(1.0, MIB, 0.0)
        })
        .collect();
    let mut detector = AlertDetector::default();
    detector.set_thresholds(Thresholds::default());
    run(&mut detector, 2, |_| many.clone());
    assert!(detector.tracked_apps() <= 256);
    run(&mut detector, 15, |_| Vec::new());
    assert_eq!(detector.tracked_apps(), 0);
}

#[test]
fn feed_merges_repeats_per_local_day() {
    let mut detector = AlertDetector::new(Thresholds {
        cooldown_seconds: 0,
        ..Thresholds::default()
    });
    let alerts = run(&mut detector, 12, |_| vec![app(80.0, GIB, 0.0)]);
    assert_eq!(alerts.len(), 3);

    let mut feed = WorthALook::new();
    for alert in &alerts {
        feed.record(alert, 0);
    }
    let tomorrow = Alert {
        at_unix: BASE_UNIX + 86_400 + 60,
        ..alerts[0].clone()
    };
    let saved = feed.record(&tomorrow, 0).clone();
    assert_eq!(saved.count, 1);

    let days = feed.days(None, 0);
    assert_eq!(days.len(), 2);
    assert_eq!(days[0].day_start_unix, BASE_UNIX + 86_400);
    let today = days[1].entries[0].record.clone();
    let id = days[1].entries[0].id;
    assert_eq!((today.count, today.end_unix - BASE_UNIX), (3, 720));
    assert!(feed.days(Some(AlertKind::DiskBurst), 0).is_empty());

    assert!(
        feed.set_pinned(id, true)
            .is_some_and(|record| record.pinned)
    );
    assert_eq!(feed.pinned().count(), 1);
    let scope = QuietScope::App(AppKey::new(AppKind::Desktop, "org.example.Editor"));
    assert_eq!(feed.mark_quieted(&scope).filter(|r| r.quieted).count(), 2);

    let busy = [app(99.0, GIB, 0.0)];
    let state = today.current_state(&input(BASE_UNIX, &busy));
    assert_eq!(state.value, Some(99.0));
    assert!(state.over_limit);
    assert_eq!(today.current_state(&input(BASE_UNIX, &[])).value, None);

    let records: Vec<_> = feed.entries().iter().map(|e| e.record.clone()).collect();
    let restored = WorthALook::from_records(records.into_iter().rev());
    assert_eq!(restored.days(None, 0).len(), 2);
    assert_eq!(AlertKind::from_code(today.kind.code()), Some(today.kind));
}

#[test]
fn quiet_until_tomorrow_ends_at_local_midnight() {
    let offset = 2 * 3600;
    let now = BASE_UNIX + 10 * 3600;
    assert_eq!(
        QuietUntil::tomorrow(now, offset),
        QuietUntil::Time {
            unix_seconds: BASE_UNIX + 86_400 - i64::from(offset)
        }
    );
}
