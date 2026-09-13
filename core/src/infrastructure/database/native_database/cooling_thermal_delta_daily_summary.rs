//! The `cooling_thermal_delta_daily_summary` projection, the paired-minute
//! read the rollup folds it from, and the pairable-ambient cursor both it and
//! the co-variate projection share - against a finalized native database.
//!
//! Runs beside
//! [`crate::infrastructure::database::cooling_thermal_delta_daily_summary`].
//!
//! The pairing rule is structural here exactly as it is in SQLite: the ambient
//! side is collapsed to one value per `(minute, source)` and then INNER JOINed
//! on the minute, so every row is one archived minute beside one sensor's
//! reading for that same minute. A minute with no ambient row for a source
//! yields no row for that source, never an interpolated one - and nothing
//! downstream can subtract two summaries built over different sample sets, or
//! since #2062 over different sensors.

use chrono::{DateTime, NaiveDate, Utc};
use duckdb::params;

use super::NativeDatabaseError;
use super::archive_sql::{
  AMBIENT_EPOCH_MS, DATA_EPOCH_MS, date_key, local_retention_cutoff, minute_key,
  preserving_delete_sql, shifted_seconds_text, union_real,
};
use super::binding::sqlite_real_binding;
use super::runtime::{NativeCancellation, NativeDatabase, NativeTransactionContext};
use super::stored_text;
use crate::persistence::cooling_rollup::BandSummary;
use crate::persistence::cooling_thermal_delta_rollup::{
  ThermalDeltaDailySummary, ThermalDeltaMinuteSample,
};

const TABLE: &str = "cooling_thermal_delta_daily_summary";
const ARCHIVE_TABLE: &str = "DATA_ARCHIVE";

/// Every `(archived minute, ambient source)` pair inside `[start, end)`,
/// oldest first and grouped by source within a minute.
///
/// The per-source `AVG` is a formality: the archive writer refuses a second
/// row for a label it already wrote this minute, so the group is one row wide
/// unless the database was edited by hand. What the `GROUP BY` must never do
/// is average *across* sources, and it does not.
pub async fn select_thermal_delta_minutes_for_range(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  start: &DateTime<Utc>,
  end: &DateTime<Utc>,
) -> Result<Vec<ThermalDeltaMinuteSample>, NativeDatabaseError> {
  let (start_ms, end_ms) = (start.timestamp_millis(), end.timestamp_millis());
  let ambient_minute_key = minute_key(AMBIENT_EPOCH_MS);
  let hardware_minute_key = minute_key(DATA_EPOCH_MS);
  let cpu_avg = union_real("DATA_ARCHIVE.cpu_avg");
  let sql = format!(
    "SELECT
       DATA_ARCHIVE.timestamp,
       ambient.source,
       ambient.ambient_temperature,
       {cpu_avg},
       DATA_ARCHIVE.cpu_temperature_avg,
       DATA_ARCHIVE.cpu_temperature_max,
       DATA_ARCHIVE.cpu_temperature_min,
       DATA_ARCHIVE.cpu_power_avg
     FROM DATA_ARCHIVE
     JOIN (
       SELECT {ambient_minute_key} AS ambient_minute_key,
              AMBIENT_ARCHIVE.source AS source,
              AVG(CAST(AMBIENT_ARCHIVE.temperature AS DOUBLE)) AS ambient_temperature
       FROM AMBIENT_ARCHIVE
       WHERE {AMBIENT_EPOCH_MS} >= {start_ms} AND {AMBIENT_EPOCH_MS} < {end_ms}
       GROUP BY ambient_minute_key, AMBIENT_ARCHIVE.source
     ) AS ambient ON ambient.ambient_minute_key = {hardware_minute_key}
     WHERE {DATA_EPOCH_MS} >= {start_ms} AND {DATA_EPOCH_MS} < {end_ms}
     ORDER BY DATA_ARCHIVE.timestamp ASC, ambient.source ASC"
  );
  database
    .request_read(cancellation, move |context| {
      context.check_cancelled()?;
      let mut statement = context.connection().prepare(&sql).map_err(|error| {
        NativeDatabaseError::duckdb("prepare the paired minute query", error)
      })?;
      let rows = statement
        .query_map([], |row| {
          Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, f64>(2)?,
            [
              row.get::<_, Option<f64>>(3)?,
              row.get::<_, Option<f64>>(4)?,
              row.get::<_, Option<f64>>(5)?,
              row.get::<_, Option<f64>>(6)?,
              row.get::<_, Option<f64>>(7)?,
            ],
          ))
        })
        .map_err(|error| {
          NativeDatabaseError::duckdb("run the paired minute query", error)
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
          NativeDatabaseError::duckdb("decode a paired minute row", error)
        })?;
      rows
        .into_iter()
        .map(|(timestamp, source, ambient_temperature, measurements)| {
          let narrow = |index: usize| measurements[index].map(|value| value as f32);
          Ok(ThermalDeltaMinuteSample {
            timestamp: stored_text::datetime(ARCHIVE_TABLE, "timestamp", &timestamp)?,
            source,
            ambient_temperature: ambient_temperature as f32,
            cpu_usage_avg: narrow(0),
            cpu_temperature_avg: narrow(1),
            cpu_temperature_max: narrow(2),
            cpu_temperature_min: narrow(3),
            cpu_power_avg: narrow(4),
          })
        })
        .collect()
    })
    .await
}

/// `MAX(date)` - the Thermal Delta projection's own catch-up cursor. A row
/// exists only for a `(day, source)` that paired at least one minute, so this
/// is exactly the latest day that recorded any ambient coverage.
pub async fn max_summarized_date(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
) -> Result<Option<NaiveDate>, NativeDatabaseError> {
  database
    .request_read(cancellation, move |context| {
      context.check_cancelled()?;
      let latest: Option<String> = context
        .connection()
        .query_row(
          "SELECT MAX(date) FROM cooling_thermal_delta_daily_summary",
          [],
          |row| row.get(0),
        )
        .map_err(|error| {
          NativeDatabaseError::duckdb("read the latest summarized ΔT day", error)
        })?;
      latest
        .map(|text| stored_text::date(TABLE, "date", &text))
        .transpose()
    })
    .await
}

/// The SQL behind [`max_pairable_ambient_archive_timestamp_before`]: the
/// latest ambient archive timestamp before `before_ms` whose minute has a
/// `DATA_ARCHIVE` row satisfying `hardware_predicate` - an extra `AND ...`
/// clause on the hardware row, or empty. The co-variate rollup's cursor
/// narrows the hardware side to classifiable minutes through it (#2068), so
/// the two cursors share one pairing rule.
///
/// The two-minute text bracket inside the `EXISTS` is kept rather than
/// dropped. In SQLite it exists to let the timestamp index be range-scanned,
/// which a native stored key does not need - but it is also the only place
/// where the SQLite answer is *approximate*, because it compares raw text
/// across writers that may differ in offset suffix. Reproducing it keeps the
/// native answer identical to the SQLite one on every database, including one
/// edited by hand into mixed spellings, rather than merely on the ones a
/// HardwareVisualizer writer produced. `strftime` over the derived key
/// normalizes the ambient side to the same `%Y-%m-%dT%H:%M:%S` prefix SQLite's
/// `strftime` does.
pub(super) fn pairable_ambient_cursor_sql(
  before_ms: i64,
  hardware_predicate: &str,
) -> String {
  let ambient_minute_key = minute_key(AMBIENT_EPOCH_MS);
  let hardware_minute_key = minute_key(DATA_EPOCH_MS);
  let lower = shifted_seconds_text(AMBIENT_EPOCH_MS, -120_000);
  let upper = shifted_seconds_text(AMBIENT_EPOCH_MS, 120_000);
  format!(
    "SELECT MAX(AMBIENT_ARCHIVE.timestamp) FROM AMBIENT_ARCHIVE
     WHERE {AMBIENT_EPOCH_MS} < {before_ms}
       AND EXISTS (
         SELECT 1 FROM DATA_ARCHIVE
         WHERE DATA_ARCHIVE.timestamp >= {lower}
           AND DATA_ARCHIVE.timestamp <= {upper}
           AND {hardware_minute_key} = {ambient_minute_key}
           {hardware_predicate}
       )"
  )
}

/// Run one of the two pairable-ambient cursors and decode its answer.
pub(super) async fn pairable_ambient_cursor(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  before: &DateTime<Utc>,
  hardware_predicate: &'static str,
  context_label: &'static str,
) -> Result<Option<DateTime<Utc>>, NativeDatabaseError> {
  let sql = pairable_ambient_cursor_sql(before.timestamp_millis(), hardware_predicate);
  database
    .request_read(cancellation, move |context| {
      context.check_cancelled()?;
      let latest: Option<String> = context
        .connection()
        .query_row(&sql, [], |row| row.get(0))
        .map_err(|error| NativeDatabaseError::duckdb(context_label, error))?;
      latest
        .map(|text| stored_text::datetime("AMBIENT_ARCHIVE", "timestamp", &text))
        .transpose()
    })
    .await
}

/// The most recent ambient archive timestamp strictly before `before` whose
/// minute also has a `DATA_ARCHIVE` row (#2045).
///
/// `before` is the start of today in local time, so the answer only ever names
/// a *completed* day: today's ambient rows are not evidence that a day was
/// missed, and counting them would rewind the catch-up on every cycle forever.
/// The `EXISTS` matches the rollup's own coverage gate - an ambient row whose
/// minute has no hardware row can never become coverage however often the day
/// is re-rolled.
pub async fn max_pairable_ambient_archive_timestamp_before(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  before: &DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, NativeDatabaseError> {
  pairable_ambient_cursor(
    database,
    cancellation,
    before,
    "",
    "read the latest pairable ambient stamp",
  )
  .await
}

/// The nineteen columns of one summarized source-day.
const COLUMNS: &str = "date, source, coverage_minutes,
  idle_delta_temperature_avg, idle_delta_temperature_max, idle_delta_temperature_min, idle_delta_sample_minutes,
  low_delta_temperature_avg, low_delta_temperature_max, low_delta_temperature_min, low_delta_sample_minutes,
  mid_delta_temperature_avg, mid_delta_temperature_max, mid_delta_temperature_min, mid_delta_sample_minutes,
  high_delta_temperature_avg, high_delta_temperature_max, high_delta_temperature_min, high_delta_sample_minutes";

/// Every summarized source-day, oldest first and grouped by source within a
/// day.
pub async fn select_all_thermal_delta_daily_summaries(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
) -> Result<Vec<ThermalDeltaDailySummary>, NativeDatabaseError> {
  let sql = format!(
    "SELECT {COLUMNS} FROM cooling_thermal_delta_daily_summary
     ORDER BY date ASC, source ASC"
  );
  database
    .request_read(cancellation, move |context| {
      context.check_cancelled()?;
      let mut statement = context.connection().prepare(&sql).map_err(|error| {
        NativeDatabaseError::duckdb("prepare the ΔT daily summary query", error)
      })?;
      let rows = statement
        .query_map([], |row| {
          // Four `avg, max, min` triples, each followed by its own count,
          // starting at column 3.
          let mut readings = [None; 12];
          for (slot, column) in readings
            .iter_mut()
            .zip([3, 4, 5, 7, 8, 9, 11, 12, 13, 15, 16, 17])
          {
            *slot = row.get::<_, Option<f64>>(column)?;
          }
          let mut counts = [0i64; 4];
          for (slot, column) in counts.iter_mut().zip([6, 10, 14, 18]) {
            *slot = row.get::<_, i64>(column)?;
          }
          Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            readings,
            counts,
          ))
        })
        .map_err(|error| {
          NativeDatabaseError::duckdb("run the ΔT daily summary query", error)
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
          NativeDatabaseError::duckdb("decode a ΔT daily summary row", error)
        })?;
      rows
        .into_iter()
        .map(|(date, source, coverage_minutes, readings, counts)| {
          let narrow = |index: usize| readings[index].map(|value| value as f32);
          let band = |first: usize, minutes: i64| BandSummary {
            avg: narrow(first),
            max: narrow(first + 1),
            min: narrow(first + 2),
            sample_minutes: stored_text::count(minutes),
          };
          Ok(ThermalDeltaDailySummary {
            date: stored_text::date(TABLE, "date", &date)?,
            source,
            coverage_minutes: stored_text::count(coverage_minutes),
            idle: band(0, counts[0]),
            low: band(3, counts[1]),
            mid: band(6, counts[2]),
            high: band(9, counts[3]),
          })
        })
        .collect()
    })
    .await
}

/// Upsert one source-day inside a caller's transaction.
pub(super) fn upsert_in(
  transaction: &NativeTransactionContext<'_, '_>,
  summary: &ThermalDeltaDailySummary,
) -> Result<(), NativeDatabaseError> {
  let minutes = |band: &BandSummary| i64::from(band.sample_minutes);
  let value = |reading: Option<f32>| {
    reading.and_then(|reading| sqlite_real_binding(f64::from(reading)))
  };
  transaction
    .connection()
    .execute(
      "INSERT INTO cooling_thermal_delta_daily_summary (
         date, source, coverage_minutes,
         idle_delta_temperature_avg, idle_delta_temperature_max, idle_delta_temperature_min, idle_delta_sample_minutes,
         low_delta_temperature_avg, low_delta_temperature_max, low_delta_temperature_min, low_delta_sample_minutes,
         mid_delta_temperature_avg, mid_delta_temperature_max, mid_delta_temperature_min, mid_delta_sample_minutes,
         high_delta_temperature_avg, high_delta_temperature_max, high_delta_temperature_min, high_delta_sample_minutes
       )
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
       ON CONFLICT (date, source) DO UPDATE SET
         coverage_minutes = excluded.coverage_minutes,
         idle_delta_temperature_avg = excluded.idle_delta_temperature_avg,
         idle_delta_temperature_max = excluded.idle_delta_temperature_max,
         idle_delta_temperature_min = excluded.idle_delta_temperature_min,
         idle_delta_sample_minutes = excluded.idle_delta_sample_minutes,
         low_delta_temperature_avg = excluded.low_delta_temperature_avg,
         low_delta_temperature_max = excluded.low_delta_temperature_max,
         low_delta_temperature_min = excluded.low_delta_temperature_min,
         low_delta_sample_minutes = excluded.low_delta_sample_minutes,
         mid_delta_temperature_avg = excluded.mid_delta_temperature_avg,
         mid_delta_temperature_max = excluded.mid_delta_temperature_max,
         mid_delta_temperature_min = excluded.mid_delta_temperature_min,
         mid_delta_sample_minutes = excluded.mid_delta_sample_minutes,
         high_delta_temperature_avg = excluded.high_delta_temperature_avg,
         high_delta_temperature_max = excluded.high_delta_temperature_max,
         high_delta_temperature_min = excluded.high_delta_temperature_min,
         high_delta_sample_minutes = excluded.high_delta_sample_minutes",
      params![
        date_key(summary.date),
        summary.source.as_str(),
        i64::from(summary.coverage_minutes),
        value(summary.idle.avg),
        value(summary.idle.max),
        value(summary.idle.min),
        minutes(&summary.idle),
        value(summary.low.avg),
        value(summary.low.max),
        value(summary.low.min),
        minutes(&summary.low),
        value(summary.mid.avg),
        value(summary.mid.max),
        value(summary.mid.min),
        minutes(&summary.mid),
        value(summary.high.avg),
        value(summary.high.max),
        value(summary.high.min),
        minutes(&summary.high),
      ],
    )
    .map_err(|error| NativeDatabaseError::duckdb("upsert a ΔT daily summary", error))?;
  Ok(())
}

/// Delete rows older than `retention_days`, except those inside any of
/// `preserved_windows` - the pinned ΔT baseline's calendar window.
pub async fn delete_old_data(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  retention_days: u32,
  preserved_windows: &[(NaiveDate, NaiveDate)],
) -> Result<(), NativeDatabaseError> {
  let sql = preserving_delete_sql(
    "cooling_thermal_delta_daily_summary",
    "date",
    preserved_windows.len(),
  );
  let mut bounds = vec![local_retention_cutoff(retention_days)];
  for (start, end) in preserved_windows {
    bounds.push(date_key(*start));
    bounds.push(date_key(*end));
  }
  database
    .request_write(cancellation, move |context| {
      context.with_transaction(|transaction| {
        transaction
          .connection()
          .execute(&sql, duckdb::params_from_iter(bounds.iter()))
          .map_err(|error| {
            NativeDatabaseError::duckdb("delete expired ΔT daily summaries", error)
          })?;
        Ok(())
      })
    })
    .await
}
