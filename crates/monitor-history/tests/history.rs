mod common;

use std::time::Instant;

use common::{
    BASE_UNIX, DISK_READ_PER_TICK, PAUSED_MINUTES, SPIKE_MINUTES, TICK_SECONDS, TempDir,
    TestResult, app_sample, two_hours_of_batches,
};
use monitor_core::{AppKey, AppKind, Bytes, DeviceKey};
use monitor_history::{
    AppMetric, GapOutcome, GpuEngine, GpuMetric, GpuMinute, HistoryError, HistoryMetric,
    HistoryReader, HistoryStore, HistoryWriter, KernelCounters, LimitKind, LimitThresholds,
    Retention, RollupBatch, Span, StoredAlert,
};

const RECORDED_TICKS: f64 = 1319.0;

fn store_with_two_hours(directory: &TempDir) -> Result<HistoryStore, HistoryError> {
    let mut store = HistoryStore::open(&directory.database(), Retention::Days30)?;
    for batch in two_hours_of_batches() {
        store.write_batch(&batch)?;
    }
    Ok(store)
}

fn two_hour_span() -> Span {
    Span::new(BASE_UNIX, BASE_UNIX + 2 * 3600)
}

#[test]
fn series_follow_recorded_minutes_and_leave_gaps() -> TestResult {
    let directory = TempDir::new("series")?;
    let _store = store_with_two_hours(&directory)?;
    let reader = HistoryReader::open(&directory.database())?;

    let cpu = reader.series(HistoryMetric::Cpu, two_hour_span())?;
    assert_eq!(cpu.len(), 120);
    for (minute, point) in (0_i64..).zip(&cpu) {
        assert_eq!(point.start_unix, BASE_UNIX + minute * 60);
        if PAUSED_MINUTES.contains(&minute) {
            assert_eq!(point.value, None, "minute {minute} was not recorded");
            continue;
        }
        let expected = if SPIKE_MINUTES.contains(&minute) {
            95.0
        } else {
            50.0
        };
        let value = point.value.ok_or("recorded minute has a value")?;
        assert!((value - expected).abs() < 1e-6, "minute {minute}: {value}");
    }

    let disk = reader.series(HistoryMetric::DiskRead, two_hour_span())?;
    let disk_rate = disk[5].value.ok_or("disk rate")?;
    assert!((disk_rate - DISK_READ_PER_TICK as f64 / TICK_SECONDS as f64).abs() < 1e-9);

    let temperature = reader.series(HistoryMetric::Temperature, two_hour_span())?;
    assert_eq!(temperature.len(), 24);
    assert_eq!(temperature[6].value, Some(90.0));
    assert_eq!(temperature[12].value, None);
    Ok(())
}

#[test]
fn totals_peaks_limits_and_coverage() -> TestResult {
    let directory = TempDir::new("totals")?;
    let _store = store_with_two_hours(&directory)?;
    let reader = HistoryReader::open(&directory.database())?;
    let span = two_hour_span();

    let totals = reader.totals(span)?;
    assert_eq!(
        totals.recorded_seconds,
        RECORDED_TICKS * TICK_SECONDS as f64
    );
    assert_eq!(
        totals.recorded.disk_read,
        Bytes::new(RECORDED_TICKS as u64 * DISK_READ_PER_TICK)
    );
    assert!(!totals.includes_estimate);
    let expected_cpu: f64 = (0..120)
        .filter(|minute| !PAUSED_MINUTES.contains(minute))
        .map(|minute| f64::from(common::cpu_percent_for_minute(minute)) / 100.0 * 60.0)
        .sum::<f64>()
        - 0.5 * TICK_SECONDS as f64;
    assert!((totals.recorded.cpu_busy_seconds - expected_cpu).abs() < 1e-6);

    let peaks = reader.peaks(span)?;
    let cpu_peak = peaks.cpu_percent.ok_or("cpu peak")?;
    assert_eq!(cpu_peak.value, 95.0);
    assert_eq!(cpu_peak.at_unix, BASE_UNIX + SPIKE_MINUTES.start * 60);
    assert_eq!(peaks.power_watts.map(|peak| peak.value), Some(15.0));

    let stretches = reader.limit_stretches(span, &LimitThresholds::default())?;
    let spike_from = BASE_UNIX + SPIKE_MINUTES.start * 60;
    let spike_to = BASE_UNIX + SPIKE_MINUTES.end * 60;
    let kinds: Vec<_> = stretches
        .iter()
        .map(|s| (s.kind, s.from_unix, s.to_unix))
        .collect();
    assert_eq!(
        kinds,
        [
            (LimitKind::Cpu, spike_from, spike_to),
            (LimitKind::Temperature, spike_from, spike_to)
        ]
    );

    let coverage = reader.coverage(span)?;
    assert!((coverage.fraction() - RECORDED_TICKS * 5.0 / 7200.0).abs() < 1e-9);
    Ok(())
}

#[test]
fn app_rankings_come_from_app_tiers() -> TestResult {
    let directory = TempDir::new("apps")?;
    let _store = store_with_two_hours(&directory)?;
    let reader = HistoryReader::open(&directory.database())?;

    let top = reader.top_apps(two_hour_span(), AppMetric::Cpu, 1)?;
    assert_eq!(top.len(), 1);
    assert_eq!(&*top[0].name, "Editor, Pro");
    assert_eq!(top[0].cpu_seconds, 2.0 * RECORDED_TICKS);
    assert_eq!(top[0].average_memory(), Bytes::new(100 << 20));

    let hours = reader.busiest_apps_by_hour(two_hour_span(), 5)?;
    assert_eq!(hours.len(), 2);
    assert!(hours.iter().all(|hour| hour.apps.len() == 2));
    assert_eq!(hours[0].apps[0].key.kind(), AppKind::Desktop);
    assert_eq!(hours[1].start_unix, BASE_UNIX + 3600);

    let week = Span::new(BASE_UNIX - 6 * 86_400, BASE_UNIX + 86_400);
    let by_day = reader.top_apps(week, AppMetric::DiskWrite, 10)?;
    assert_eq!(by_day.len(), 2);
    Ok(())
}

#[test]
fn per_device_and_per_app_series() -> TestResult {
    let directory = TempDir::new("devices")?;
    let _store = store_with_two_hours(&directory)?;
    let reader = HistoryReader::open(&directory.database())?;
    let span = two_hour_span();

    let gpu = DeviceKey::new("0000:06:00.0");
    let busy = reader.gpu_series(&gpu, GpuMetric::Busy, span)?;
    assert_eq!(busy.len(), 24, "five-minute buckets up to 48 h");
    let recorded: Vec<_> = busy.iter().filter_map(|point| point.value).collect();
    assert_eq!(recorded.len(), 22, "the ten-minute pause stays a gap");
    assert!(recorded.iter().all(|percent| (percent - 10.0).abs() < 1e-9));
    let vram = reader.gpu_series(&gpu, GpuMetric::VramUsed, span)?;
    assert_eq!(vram[0].value, Some(f64::from(1_u32 << 30)));
    let three_days = Span::new(span.from_unix, span.from_unix + 3 * 86_400);
    let hourly = reader.gpu_series(&gpu, GpuMetric::Busy, three_days)?;
    assert_eq!(hourly.len(), 72, "hourly buckets beyond 48 h");
    assert!(hourly[..2].iter().all(|point| point.value == Some(10.0)));
    let other = reader.gpu_series(&DeviceKey::new("0000:01:00.0"), GpuMetric::Busy, span)?;
    assert!(other.iter().all(|point| point.value.is_none()));

    let engines = reader.gpu_engine_series(&gpu, span)?;
    let reported: Vec<_> = engines.iter().map(|series| series.engine).collect();
    assert_eq!(reported, [GpuEngine::Graphics, GpuEngine::Video]);
    let graphics = &engines[0].points;
    assert_eq!(graphics.len(), 24, "five-minute buckets up to 48 h");
    let recorded: Vec<_> = graphics.iter().filter_map(|point| point.value).collect();
    assert_eq!(recorded.len(), 22, "the ten-minute pause stays a gap");
    assert!(recorded.iter().all(|percent| (percent - 10.0).abs() < 1e-9));
    assert!(
        engines[1]
            .points
            .iter()
            .filter_map(|point| point.value)
            .all(|percent| (percent - 20.0).abs() < 1e-9)
    );
    assert!(
        reader.gpu_engine_series(&gpu, three_days)?[0]
            .points
            .iter()
            .take(2)
            .all(|point| point
                .value
                .is_some_and(|percent| (percent - 10.0).abs() < 1e-9)),
        "hourly tier beyond 48 h"
    );
    assert!(
        reader
            .gpu_engine_series(&DeviceKey::new("0000:01:00.0"), span)?
            .is_empty()
    );

    let editor = AppKey::new(AppKind::Desktop, "org.example.Editor");
    let memory = reader.app_memory_series(&editor, span)?;
    assert_eq!(memory.len(), 2);
    assert!(
        memory
            .iter()
            .all(|point| point.value == Some(f64::from(100_u32 << 20)))
    );

    let interfaces = reader.interface_totals(span)?;
    assert_eq!(interfaces.len(), 1);
    assert_eq!(&*interfaces[0].interface, "wlo1");
    assert_eq!(
        interfaces[0].received,
        Bytes::new(500 * RECORDED_TICKS as u64)
    );
    Ok(())
}

#[test]
fn same_boot_gap_is_estimated_and_reboot_is_not_recorded() -> TestResult {
    let mut store = HistoryStore::open_in_memory(Retention::Days30)?;
    let first = KernelCounters {
        taken_unix_seconds: BASE_UNIX,
        boot_id: "boot-a".into(),
        cpu_busy_seconds_total: 100.0,
        disk_read_total: Bytes::new(1_000),
        disk_written_total: Bytes::new(2_000),
        network_received_total: Bytes::new(5_000),
        network_transmitted_total: Bytes::new(6_000),
    };
    assert_eq!(store.start_session(&first)?, GapOutcome::FirstRun);
    let reopened = KernelCounters {
        taken_unix_seconds: BASE_UNIX + 3600,
        cpu_busy_seconds_total: 400.0,
        disk_read_total: Bytes::new(11_000),
        network_received_total: Bytes::new(4_000),
        ..first.clone()
    };
    assert_eq!(store.start_session(&reopened)?, GapOutcome::Estimated);
    let rebooted = KernelCounters {
        taken_unix_seconds: BASE_UNIX + 7200,
        boot_id: "boot-b".into(),
        ..KernelCounters::default()
    };
    assert_eq!(store.start_session(&rebooted)?, GapOutcome::NotRecorded);

    let directory = TempDir::new("gaps")?;
    let path = directory.database();
    std::fs::create_dir_all(path.parent().ok_or("parent")?)?;
    store
        .connection()
        .execute("VACUUM INTO ?1", [path.to_string_lossy()])?;
    let reader = HistoryReader::open(&path)?;
    let gaps = reader.gaps(Span::new(BASE_UNIX, BASE_UNIX + 7200))?;
    assert_eq!(gaps.len(), 2);
    assert!(gaps[0].estimated);
    assert_eq!(gaps[0].cpu_busy_seconds, Some(300.0));
    assert_eq!(gaps[0].disk_read, Some(Bytes::new(10_000)));
    assert_eq!(gaps[0].disk_written, Some(Bytes::ZERO));
    assert_eq!(gaps[0].network_received, None);
    assert!(!gaps[1].estimated);
    assert_eq!(
        (gaps[1].start_unix, gaps[1].end_unix),
        (BASE_UNIX + 3600, BASE_UNIX + 7200)
    );

    let half_of_gap = reader.totals(Span::new(BASE_UNIX + 1800, BASE_UNIX + 3600))?;
    assert!(half_of_gap.includes_estimate);
    assert_eq!(half_of_gap.estimated.cpu_busy_seconds, 150.0);
    assert_eq!(half_of_gap.combined().disk_read, Bytes::new(5_000));
    assert_eq!(reader.recording_since()?, Some(BASE_UNIX));
    Ok(())
}

#[test]
fn retention_prunes_old_rows_and_orphan_apps() -> TestResult {
    let directory = TempDir::new("retention")?;
    let mut store = HistoryStore::open(&directory.database(), Retention::Days30)?;
    let old_start = BASE_UNIX - 40 * 86_400;
    let mut old = minute_batch(old_start, "org.example.Old");
    old.gpus.clear();
    store.write_batch(&old)?;
    store.write_batch(&minute_batch(BASE_UNIX, "org.example.New"))?;

    let reader = HistoryReader::open(&directory.database())?;
    let everything = Span::new(old_start - 86_400, BASE_UNIX + 86_400);
    let names: Vec<_> = reader
        .top_apps(everything, AppMetric::Cpu, 10)?
        .into_iter()
        .map(|usage| usage.name)
        .collect();
    assert_eq!(names, [std::sync::Arc::from("New")]);
    let orphans: i64 = store.connection().query_row(
        "SELECT count(*) FROM apps WHERE app_key LIKE '%Old'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(orphans, 0);
    let old_minutes: i64 = store.connection().query_row(
        "SELECT count(*) FROM system_minutes WHERE start_unix < ?1",
        [BASE_UNIX - 3 * 3600],
        |row| row.get(0),
    )?;
    assert_eq!(old_minutes, 0);

    store.set_retention(Retention::Days90);
    store.write_batch(&minute_batch(old_start, "org.example.Old"))?;
    store.prune_now(BASE_UNIX)?;
    assert_eq!(reader.top_apps(everything, AppMetric::Cpu, 10)?.len(), 2);
    Ok(())
}

#[test]
fn migration_is_idempotent_and_rejects_newer_schema() -> TestResult {
    let directory = TempDir::new("schema")?;
    let path = directory.database();
    drop(HistoryStore::open(&path, Retention::Days30)?);
    let store = HistoryStore::open(&path, Retention::Days30)?;
    let version: i32 = store
        .connection()
        .pragma_query_value(None, "user_version", |row| row.get(0))?;
    let auto_vacuum: i32 = store
        .connection()
        .pragma_query_value(None, "auto_vacuum", |row| row.get(0))?;
    let journal: String = store
        .connection()
        .pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    assert_eq!((version, auto_vacuum, journal.as_str()), (2, 2, "wal"));
    store.connection().pragma_update(None, "user_version", 99)?;
    drop(store);
    assert!(matches!(
        HistoryStore::open(&path, Retention::Days30),
        Err(HistoryError::UnsupportedSchema { found: 99, .. })
    ));
    Ok(())
}

#[test]
fn csv_export_writes_three_quoted_files() -> TestResult {
    let directory = TempDir::new("csv")?;
    let _store = store_with_two_hours(&directory)?;
    let reader = HistoryReader::open(&directory.database())?;
    let files = reader.export_csv(Some(directory.path()), two_hour_span())?;
    assert_eq!(files.len(), 3);
    let apps = std::fs::read_to_string(&files[0])?;
    let mut lines = apps.lines();
    assert!(
        lines
            .next()
            .ok_or("header")?
            .starts_with("day_utc,app_key,name,kind,")
    );
    assert!(apps.contains(",\"Editor, Pro\",desktop,"));
    let hours = std::fs::read_to_string(&files[1])?;
    assert_eq!(hours.lines().count(), 3);
    let readings = std::fs::read_to_string(&files[2])?;
    assert_eq!(readings.lines().count(), 1 + 22);
    Ok(())
}

#[test]
fn alerts_round_trip() -> TestResult {
    let directory = TempDir::new("alerts")?;
    let mut store = HistoryStore::open(&directory.database(), Retention::Days30)?;
    let mut alert = StoredAlert {
        kind: "sustained_cpu".into(),
        app_key: Some(AppKey::new(AppKind::Desktop, "org.example.Editor")),
        app_name: Some("Editor".into()),
        start_unix: BASE_UNIX,
        end_unix: BASE_UNIX,
        peak: 88.0,
        limit: 70.0,
        count: 1,
        quieted: false,
        pinned: false,
    };
    store.save_alert(&alert)?;
    alert.count = 3;
    alert.end_unix += 7200;
    alert.pinned = true;
    store.save_alert(&alert)?;
    store.save_alert(&StoredAlert {
        kind: "machine_hot".into(),
        app_key: None,
        app_name: None,
        ..alert.clone()
    })?;
    let reader = HistoryReader::open(&directory.database())?;
    let loaded = reader.alerts(Span::new(BASE_UNIX, BASE_UNIX + 86_400))?;
    assert_eq!(loaded.len(), 2);
    assert!(loaded.contains(&alert));
    assert!(
        loaded
            .iter()
            .any(|row| row.app_key.is_none() && row.app_name.is_none())
    );
    Ok(())
}

#[test]
fn writer_thread_persists_and_deletes() -> TestResult {
    let directory = TempDir::new("writer")?;
    let writer = HistoryWriter::spawn(&directory.database(), Retention::Days30)?;
    let counters = KernelCounters {
        taken_unix_seconds: BASE_UNIX,
        boot_id: "boot".into(),
        ..KernelCounters::default()
    };
    let outcome = writer.record_session_start(counters.clone())?.recv()??;
    assert_eq!(outcome, GapOutcome::FirstRun);
    let batches = two_hours_of_batches();
    let submitted = batches.len();
    let queued = batches
        .into_iter()
        .map(|batch| writer.submit(batch))
        .filter(|queued| *queued)
        .count();
    assert_eq!(queued as u64 + writer.dropped_batches(), submitted as u64);
    let _ = writer.checkpoint(counters);
    let reader = HistoryReader::open(&directory.database())?;
    writer.set_retention(Retention::Year1)?;
    writer.delete_all()?.recv()??;
    assert_eq!(reader.totals(two_hour_span())?.recorded_seconds, 0.0);
    writer.submit(minute_batch(BASE_UNIX, "org.example.Late"));
    writer.shutdown()?;
    assert_eq!(reader.totals(two_hour_span())?.recorded_seconds, 60.0);
    Ok(())
}

#[test]
fn measures_write_cost_per_minute() -> TestResult {
    const APPS: usize = 100;
    const MINUTES: i64 = 60;
    let directory = TempDir::new("cost")?;
    let mut store = HistoryStore::open(&directory.database(), Retention::Days30)?;
    let batches: Vec<_> = (0..MINUTES)
        .map(|minute| {
            let mut batch = minute_batch(BASE_UNIX + minute * 60, "org.example.Base");
            batch.apps = (0..APPS)
                .map(|index| app_minute(&format!("org.example.App{index}")))
                .collect();
            batch
        })
        .collect();
    store.write_batch(&batches[0])?;
    let started = Instant::now();
    for batch in &batches[1..] {
        store.write_batch(batch)?;
    }
    let per_minute = started.elapsed() / u32::try_from(MINUTES - 1)?;
    eprintln!("write cost per minute with {APPS} apps: {per_minute:?}");
    assert!(per_minute.as_millis() < 250);
    Ok(())
}

fn app_minute(id: &str) -> monitor_history::AppMinute {
    let sample = app_sample(
        AppKind::Desktop,
        id,
        id.rsplit('.').next().unwrap_or(id),
        3.0,
    );
    monitor_history::AppMinute {
        key: sample.key,
        name: sample.name,
        seconds: 60.0,
        cpu_seconds: sample.cpu_seconds,
        memory_byte_seconds: sample.memory.get() as f64 * 60.0,
        peak_memory: sample.memory,
        disk_read: sample.disk_read,
        disk_written: sample.disk_written,
        network_received: sample.network_received,
        network_transmitted: sample.network_transmitted,
        gpu_seconds: 0.0,
        energy_joules: Some(1.0),
    }
}

fn minute_batch(start_unix: i64, app_id: &str) -> RollupBatch {
    let mut batch = RollupBatch::default();
    batch.system.start_unix = start_unix;
    batch.system.last_sample_unix = start_unix + 59;
    batch.system.recorded_seconds = 60.0;
    batch.system.cpu_busy_seconds = 6.0;
    batch.system.gauges[monitor_history::GaugeMetric::CpuPercent].add(10.0, 60.0);
    batch.apps.push(app_minute(app_id));
    batch.gpus.push(GpuMinute {
        key: DeviceKey::new("0000:01:00.0"),
        seconds: 60.0,
        busy_seconds: 6.0,
        engines: monitor_history::EngineTimes::default(),
        vram_byte_seconds: 0.0,
        energy_joules: None,
        peak_celsius: Some(50.0),
    });
    batch
}
