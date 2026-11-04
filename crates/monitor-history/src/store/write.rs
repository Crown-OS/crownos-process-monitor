use std::collections::HashMap;

use monitor_core::AppKey;
use monitor_core::util::time::{
    DAY_SECONDS, FIVE_MINUTES_SECONDS, HOUR_SECONDS, MINUTE_SECONDS, align_down,
};
use rusqlite::{Transaction, params};

use crate::batch::{AppMinute, RollupBatch, SystemMinute};
use crate::error::HistoryResult;
use crate::store::HistoryStore;
use crate::store::table::{
    APP_TABLES, GPU_ENGINE_TABLES, GPU_TABLES, SYSTEM_TABLES, app_table, device_table,
    gpu_engine_table, gpu_table, interface_table, readings_table, system_table,
};
use crate::util::sql::bytes_to_integer;

const SYSTEM_STEPS: [i64; 3] = [MINUTE_SECONDS, FIVE_MINUTES_SECONDS, HOUR_SECONDS];
const APP_STEPS: [i64; 2] = [HOUR_SECONDS, DAY_SECONDS];
/// The steps of [`GPU_TABLES`] and [`GPU_ENGINE_TABLES`].
const GPU_STEPS: [i64; 2] = [FIVE_MINUTES_SECONDS, HOUR_SECONDS];
const READINGS_FIXED_PARAMETERS: usize = 5;

const APP_ID_UPSERT: &str = "INSERT INTO apps (app_key, name, kind) VALUES (?1, ?2, ?3) \
    ON CONFLICT (app_key) DO UPDATE SET name = excluded.name RETURNING app_id";
const SESSION_EXTEND: &str = "INSERT INTO sessions (start_unix, end_unix, boot_id) VALUES (?1, ?2, '') \
    ON CONFLICT (start_unix) DO UPDATE SET end_unix = max(end_unix, excluded.end_unix)";

/// Upsert statements generated once from the table specs.
pub struct WriteSql {
    system: [String; 3],
    readings: String,
    apps: [String; 2],
    gpus: [String; 2],
    gpu_engines: [String; 2],
    device: String,
    interface: String,
}

impl WriteSql {
    pub fn new() -> Self {
        Self {
            system: SYSTEM_TABLES.map(|name| system_table(name).upsert_sql()),
            readings: readings_table().upsert_sql(),
            apps: APP_TABLES.map(|name| app_table(name).upsert_sql()),
            gpus: GPU_TABLES.map(|name| gpu_table(name).upsert_sql()),
            gpu_engines: GPU_ENGINE_TABLES.map(|name| gpu_engine_table(name).upsert_sql()),
            device: device_table().upsert_sql(),
            interface: interface_table().upsert_sql(),
        }
    }
}

impl HistoryStore {
    /// Writes one minute into every tier in a single transaction, extends the
    /// current session and prunes when due.
    pub fn write_batch(&mut self, batch: &RollupBatch) -> HistoryResult<()> {
        let result = self.write_batch_transaction(batch);
        if result.is_err() {
            self.app_ids.clear();
        }
        result
    }

    fn write_batch_transaction(&mut self, batch: &RollupBatch) -> HistoryResult<()> {
        let now = batch.system.last_sample_unix;
        let session_start = *self
            .session_start_unix
            .get_or_insert(batch.system.start_unix);
        let transaction = self.connection.transaction()?;
        write_system(&transaction, &self.sql, &batch.system)?;
        for app in &batch.apps {
            let app_id = app_id(&transaction, &mut self.app_ids, app)?;
            write_app(
                &transaction,
                &self.sql,
                app_id,
                batch.system.start_unix,
                app,
            )?;
        }
        write_devices(&transaction, &self.sql, batch)?;
        transaction
            .prepare_cached(SESSION_EXTEND)?
            .execute(params![session_start, now])?;
        let pruned_retained = self
            .prune_schedule
            .run_due(&transaction, now, self.retention)?;
        transaction.commit()?;
        if pruned_retained {
            self.app_ids.clear();
            self.connection
                .execute_batch("PRAGMA incremental_vacuum;")?;
        }
        Ok(())
    }
}

fn write_system(
    transaction: &Transaction<'_>,
    sql: &WriteSql,
    minute: &SystemMinute,
) -> rusqlite::Result<()> {
    for (statement, step) in sql.system.iter().zip(SYSTEM_STEPS) {
        transaction.prepare_cached(statement)?.execute(params![
            align_down(minute.start_unix, step),
            minute.recorded_seconds,
            minute.cpu_busy_seconds,
            minute.memory_byte_seconds,
            bytes_to_integer(minute.disk_read),
            bytes_to_integer(minute.disk_written),
            bytes_to_integer(minute.network_received),
            bytes_to_integer(minute.network_transmitted),
            minute.energy_joules,
        ])?;
    }
    let mut readings = transaction.prepare_cached(&sql.readings)?;
    readings.raw_bind_parameter(1, align_down(minute.start_unix, FIVE_MINUTES_SECONDS))?;
    readings.raw_bind_parameter(2, minute.recorded_seconds)?;
    readings.raw_bind_parameter(3, minute.pressure_level_peak.as_code())?;
    readings.raw_bind_parameter(4, minute.throttled_seconds)?;
    readings.raw_bind_parameter(5, minute.on_ac_seconds)?;
    for (index, (_, gauge)) in minute.gauges.iter().enumerate() {
        let average_parameter = READINGS_FIXED_PARAMETERS + 1 + 2 * index;
        readings.raw_bind_parameter(average_parameter, gauge.average())?;
        readings.raw_bind_parameter(average_parameter + 1, gauge.peak())?;
    }
    readings.raw_execute()?;
    Ok(())
}

fn app_id(
    transaction: &Transaction<'_>,
    cache: &mut HashMap<AppKey, i64>,
    app: &AppMinute,
) -> rusqlite::Result<i64> {
    if let Some(&id) = cache.get(&app.key) {
        return Ok(id);
    }
    let id = transaction.prepare_cached(APP_ID_UPSERT)?.query_row(
        params![app.key.as_str(), &*app.name, app.key.kind().prefix()],
        |row| row.get(0),
    )?;
    cache.insert(app.key.clone(), id);
    Ok(id)
}

fn write_app(
    transaction: &Transaction<'_>,
    sql: &WriteSql,
    app_id: i64,
    minute_start: i64,
    app: &AppMinute,
) -> rusqlite::Result<()> {
    for (statement, step) in sql.apps.iter().zip(APP_STEPS) {
        transaction.prepare_cached(statement)?.execute(params![
            align_down(minute_start, step),
            app_id,
            app.seconds,
            app.cpu_seconds,
            app.memory_byte_seconds,
            bytes_to_integer(app.peak_memory),
            bytes_to_integer(app.disk_read),
            bytes_to_integer(app.disk_written),
            bytes_to_integer(app.network_received),
            bytes_to_integer(app.network_transmitted),
            app.gpu_seconds,
            app.energy_joules,
        ])?;
    }
    Ok(())
}

fn write_devices(
    transaction: &Transaction<'_>,
    sql: &WriteSql,
    batch: &RollupBatch,
) -> rusqlite::Result<()> {
    let hour = align_down(batch.system.start_unix, HOUR_SECONDS);
    for gpu in &batch.gpus {
        for (statement, step) in sql.gpus.iter().zip(GPU_STEPS) {
            transaction.prepare_cached(statement)?.execute(params![
                align_down(batch.system.start_unix, step),
                gpu.key.as_str(),
                gpu.seconds,
                gpu.busy_seconds,
                gpu.vram_byte_seconds,
                gpu.energy_joules,
                gpu.peak_celsius.map(f64::from),
            ])?;
        }
        for (engine, time) in gpu.engines.iter() {
            for (statement, step) in sql.gpu_engines.iter().zip(GPU_STEPS) {
                transaction.prepare_cached(statement)?.execute(params![
                    align_down(batch.system.start_unix, step),
                    gpu.key.as_str(),
                    engine.stored_name(),
                    time.seconds,
                    time.busy_seconds,
                ])?;
            }
        }
    }
    for device in &batch.devices {
        transaction.prepare_cached(&sql.device)?.execute(params![
            hour,
            device.key.as_str(),
            bytes_to_integer(device.read),
            bytes_to_integer(device.written),
        ])?;
    }
    for interface in &batch.interfaces {
        transaction
            .prepare_cached(&sql.interface)?
            .execute(params![
                hour,
                interface.key.as_str(),
                bytes_to_integer(interface.received),
                bytes_to_integer(interface.transmitted),
            ])?;
    }
    Ok(())
}
