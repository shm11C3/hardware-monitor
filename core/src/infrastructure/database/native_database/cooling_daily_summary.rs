//! The `cooling_daily_summary` projection, and the `DATA_ARCHIVE` reads the
//! cooling rollup folds it from, against a finalized native database.
//!
//! Runs beside [`crate::infrastructure::database::cooling_daily_summary`],
//! never instead of it.
//!
//! Three differences from the SQLite module, all of them consequences of the
//! finalized schema rather than choices:
//!
//! - **The raw-TEXT pre-filter is gone.** In SQLite the range reads carry a
//!   widened `timestamp >= ?` bracket beside the exact predicate purely so the
//!   timestamp index can be range-scanned; the computed `strftime` expression
//!   cannot be. Natively the exact predicate *is* a stored column
//!   (`__hv_timestamp_epoch_ms`), so there is nothing to hint around. Dropping
//!   it cannot change which rows come back - the bracket was chosen to be
//!   incapable of excluding a row the exact predicate keeps.
//! - **`CAST(cpu_avg AS REAL)`** reads a `UNION(i BIGINT, r DOUBLE)` column
//!   natively, so it goes through [`union_real`]. The temperature and power
//!   columns are plain DOUBLE in the native schema and need no such handling.
//! - **`date` is read back as text** and decoded with the same grammar the
//!   sqlx reader uses ([`stored_text::date`]), because there is no sqlx codec
//!   on this side to hand the cell to.

use chrono::{DateTime, NaiveDate, Utc};
use duckdb::params;

use super::NativeDatabaseError;
use super::archive_sql::{
  DATA_EPOCH_MS, date_key, local_retention_cutoff, preserving_delete_sql, union_real,
};
use super::binding::sqlite_real_binding;
use super::runtime::{NativeCancellation, NativeDatabase, NativeTransactionContext};
use super::stored_text;
use crate::persistence::cooling_baseline::DailyIdleSample;
use crate::persistence::cooling_rollup::{
  ArchiveMinuteSample, BandSummary, DailyCoolingSummary, PowerSummary,
};

const TABLE: &str = "cooling_daily_summary";
const ARCHIVE_TABLE: &str = "DATA_ARCHIVE";

/// Every archived minute in `[start, end)`, oldest first, for the daily
/// rollup's own pass.
///
/// The ambient side is deliberately not joined here, exactly as in SQLite: one
/// per-minute ambient value averaged across sources would mix two sensor
/// placements into a ΔT no sensor observed (#2062), so the Thermal Delta is
/// folded per source from its own paired read.
pub async fn select_archive_minutes_for_range(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  start: &DateTime<Utc>,
  end: &DateTime<Utc>,
) -> Result<Vec<ArchiveMinuteSample>, NativeDatabaseError> {
  let (start_ms, end_ms) = (start.timestamp_millis(), end.timestamp_millis());
  let cpu_avg = union_real("DATA_ARCHIVE.cpu_avg");
  let sql = format!(
    "SELECT
       DATA_ARCHIVE.timestamp,
       {cpu_avg},
       DATA_ARCHIVE.cpu_temperature_avg,
       DATA_ARCHIVE.cpu_temperature_max,
       DATA_ARCHIVE.cpu_temperature_min,
       DATA_ARCHIVE.cpu_power_avg,
       DATA_ARCHIVE.cpu_power_max,
       DATA_ARCHIVE.cpu_power_min
     FROM DATA_ARCHIVE
     WHERE {DATA_EPOCH_MS} >= {start_ms} AND {DATA_EPOCH_MS} < {end_ms}
     ORDER BY DATA_ARCHIVE.timestamp ASC"
  );
  database
    .request_read(cancellation, move |context| {
      context.check_cancelled()?;
      let mut statement = context.connection().prepare(&sql).map_err(|error| {
        NativeDatabaseError::duckdb("prepare the archive minute query", error)
      })?;
      let rows = statement
        .query_map([], |row| {
          Ok((
            row.get::<_, String>(0)?,
            [
              row.get::<_, Option<f64>>(1)?,
              row.get::<_, Option<f64>>(2)?,
              row.get::<_, Option<f64>>(3)?,
              row.get::<_, Option<f64>>(4)?,
              row.get::<_, Option<f64>>(5)?,
              row.get::<_, Option<f64>>(6)?,
              row.get::<_, Option<f64>>(7)?,
            ],
          ))
        })
        .map_err(|error| {
          NativeDatabaseError::duckdb("run the archive minute query", error)
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
          NativeDatabaseError::duckdb("decode an archive minute row", error)
        })?;
      rows
        .into_iter()
        .map(|(timestamp, measurements)| {
          let narrow = |index: usize| measurements[index].map(|value| value as f32);
          Ok(ArchiveMinuteSample {
            timestamp: stored_text::datetime(ARCHIVE_TABLE, "timestamp", &timestamp)?,
            cpu_usage_avg: narrow(0),
            cpu_temperature_avg: narrow(1),
            cpu_temperature_max: narrow(2),
            cpu_temperature_min: narrow(3),
            cpu_power_avg: narrow(4),
            cpu_power_max: narrow(5),
            cpu_power_min: narrow(6),
          })
        })
        .collect()
    })
    .await
}

/// The latest summarized day, or `None` when nothing has been summarized.
pub async fn max_summarized_date(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
) -> Result<Option<NaiveDate>, NativeDatabaseError> {
  max_date_matching(
    database,
    cancellation,
    "SELECT MAX(date) FROM cooling_daily_summary",
    "read the latest summarized cooling day",
  )
  .await
}

/// The latest summarized day that recorded at least one minute carrying both
/// a CPU usage and a CPU temperature reading - the equivalence the catch-up
/// cursor rests on (see `cooling_rollup::rollup_catch_up_cursor`).
pub async fn max_pairable_summarized_date(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
) -> Result<Option<NaiveDate>, NativeDatabaseError> {
  max_date_matching(
    database,
    cancellation,
    "SELECT MAX(date) FROM cooling_daily_summary
     WHERE idle_sample_minutes + low_sample_minutes
         + mid_sample_minutes + high_sample_minutes > 0",
    "read the latest pairable summarized cooling day",
  )
  .await
}

/// The latest summarized day that recorded any CPU package power (#2021).
pub async fn max_powered_summarized_date(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
) -> Result<Option<NaiveDate>, NativeDatabaseError> {
  max_date_matching(
    database,
    cancellation,
    "SELECT MAX(date) FROM cooling_daily_summary WHERE power_sample_minutes > 0",
    "read the latest powered summarized cooling day",
  )
  .await
}

/// `MAX(date)` over `cooling_daily_summary`, decoded the way the SQLite reader
/// decodes it. Shared by the three cursors above so one decoding rule covers
/// all of them.
async fn max_date_matching(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  sql: &'static str,
  context_label: &'static str,
) -> Result<Option<NaiveDate>, NativeDatabaseError> {
  database
    .request_read(cancellation, move |context| {
      context.check_cancelled()?;
      let latest: Option<String> = context
        .connection()
        .query_row(sql, [], |row| row.get(0))
        .map_err(|error| NativeDatabaseError::duckdb(context_label, error))?;
      latest
        .map(|text| stored_text::date(TABLE, "date", &text))
        .transpose()
    })
    .await
}

/// The oldest archived instant, or `None` on an empty archive.
///
/// `MIN(timestamp)` over the stored text, a byte-wise minimum in both engines,
/// then decoded - the same order the SQLite reader works in.
pub async fn earliest_archived_timestamp(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
) -> Result<Option<DateTime<Utc>>, NativeDatabaseError> {
  database
    .request_read(cancellation, move |context| {
      context.check_cancelled()?;
      let earliest: Option<String> = context
        .connection()
        .query_row("SELECT MIN(timestamp) FROM DATA_ARCHIVE", [], |row| {
          row.get(0)
        })
        .map_err(|error| {
          NativeDatabaseError::duckdb("read the earliest archived timestamp", error)
        })?;
      earliest
        .map(|text| stored_text::datetime(ARCHIVE_TABLE, "timestamp", &text))
        .transpose()
    })
    .await
}

/// The most recent archived timestamp strictly before `before` whose row
/// carries a full CPU package power triple (#2021).
///
/// All three columns are required, matching `summarize_day`'s own power gate:
/// a partial triple contributes nothing there, so it must not count as power
/// the rollup failed to pick up either.
pub async fn max_powered_archive_timestamp_before(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  before: &DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, NativeDatabaseError> {
  let before_ms = before.timestamp_millis();
  let sql = format!(
    "SELECT MAX(timestamp) FROM DATA_ARCHIVE
     WHERE {DATA_EPOCH_MS} < {before_ms}
       AND cpu_power_avg IS NOT NULL
       AND cpu_power_max IS NOT NULL
       AND cpu_power_min IS NOT NULL"
  );
  database
    .request_read(cancellation, move |context| {
      context.check_cancelled()?;
      let latest: Option<String> = context
        .connection()
        .query_row(&sql, [], |row| row.get(0))
        .map_err(|error| {
          NativeDatabaseError::duckdb("read the latest powered archive stamp", error)
        })?;
      latest
        .map(|text| stored_text::datetime(ARCHIVE_TABLE, "timestamp", &text))
        .transpose()
    })
    .await
}

/// Every summarized day's idle-band facts, oldest first, for the cooling
/// baseline derivation. Reads the whole table for the same reason SQLite does:
/// it holds at most one retention window of three narrow columns, and the
/// baseline is defined over the *first* qualifying days.
pub async fn select_daily_idle_samples(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
) -> Result<Vec<DailyIdleSample>, NativeDatabaseError> {
  database
    .request_read(cancellation, move |context| {
      context.check_cancelled()?;
      let mut statement = context
        .connection()
        .prepare(
          "SELECT date, idle_cpu_temperature_avg, idle_sample_minutes
           FROM cooling_daily_summary
           ORDER BY date ASC",
        )
        .map_err(|error| {
          NativeDatabaseError::duckdb("prepare the daily idle sample query", error)
        })?;
      let rows = statement
        .query_map([], |row| {
          Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<f64>>(1)?,
            row.get::<_, i64>(2)?,
          ))
        })
        .map_err(|error| {
          NativeDatabaseError::duckdb("run the daily idle sample query", error)
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
          NativeDatabaseError::duckdb("decode a daily idle sample row", error)
        })?;
      rows
        .into_iter()
        .map(|(date, idle_temperature_avg, idle_sample_minutes)| {
          Ok(DailyIdleSample {
            date: stored_text::date(TABLE, "date", &date)?,
            idle_temperature_avg: idle_temperature_avg.map(|value| value as f32),
            idle_sample_minutes: stored_text::count(idle_sample_minutes),
          })
        })
        .collect()
    })
    .await
}

/// The twenty-two columns of one summarized day, in the order both the read
/// and the upsert use them.
const COLUMNS: &str = "date,
  idle_cpu_temperature_avg, idle_cpu_temperature_max, idle_cpu_temperature_min, idle_sample_minutes,
  low_cpu_temperature_avg, low_cpu_temperature_max, low_cpu_temperature_min, low_sample_minutes,
  mid_cpu_temperature_avg, mid_cpu_temperature_max, mid_cpu_temperature_min, mid_sample_minutes,
  high_cpu_temperature_avg, high_cpu_temperature_max, high_cpu_temperature_min, high_sample_minutes,
  coverage_minutes,
  cpu_power_avg, cpu_power_max, cpu_power_min, power_sample_minutes";

/// Every summarized day's full band breakdown, oldest first, for Cooling
/// Insight's long-range trend and load-band comparison queries (#2017).
pub async fn select_all_daily_cooling_summaries(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
) -> Result<Vec<DailyCoolingSummary>, NativeDatabaseError> {
  let sql = format!("SELECT {COLUMNS} FROM cooling_daily_summary ORDER BY date ASC");
  database
    .request_read(cancellation, move |context| {
      context.check_cancelled()?;
      let mut statement = context.connection().prepare(&sql).map_err(|error| {
        NativeDatabaseError::duckdb("prepare the daily cooling summary query", error)
      })?;
      let rows = statement
        .query_map([], |row| {
          // The five measurement groups are `avg, max, min` triples at the
          // column offsets below, each followed by its own count. Reading
          // them by explicit index keeps the two arrays in the order the
          // reconstruction below expects.
          let mut readings = [None; 15];
          for (slot, column) in readings
            .iter_mut()
            .zip([1, 2, 3, 5, 6, 7, 9, 10, 11, 13, 14, 15, 18, 19, 20])
          {
            *slot = row.get::<_, Option<f64>>(column)?;
          }
          let mut counts = [0i64; 6];
          for (slot, column) in counts.iter_mut().zip([4, 8, 12, 16, 17, 21]) {
            *slot = row.get::<_, i64>(column)?;
          }
          Ok((row.get::<_, String>(0)?, readings, counts))
        })
        .map_err(|error| {
          NativeDatabaseError::duckdb("run the daily cooling summary query", error)
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
          NativeDatabaseError::duckdb("decode a daily cooling summary row", error)
        })?;
      rows
        .into_iter()
        .map(|(date, readings, counts)| {
          let narrow = |index: usize| readings[index].map(|value| value as f32);
          let band = |first: usize, minutes: i64| BandSummary {
            avg: narrow(first),
            max: narrow(first + 1),
            min: narrow(first + 2),
            sample_minutes: stored_text::count(minutes),
          };
          Ok(DailyCoolingSummary {
            date: stored_text::date(TABLE, "date", &date)?,
            coverage_minutes: stored_text::count(counts[4]),
            idle: band(0, counts[0]),
            low: band(3, counts[1]),
            mid: band(6, counts[2]),
            high: band(9, counts[3]),
            power: PowerSummary {
              avg: narrow(12),
              max: narrow(13),
              min: narrow(14),
              sample_minutes: stored_text::count(counts[5]),
            },
          })
        })
        .collect()
    })
    .await
}

/// Upsert one summarized day inside a caller's transaction, so a whole day's
/// projections commit together (see [`super::cooling_rollup`]).
pub(super) fn upsert_in(
  transaction: &NativeTransactionContext<'_, '_>,
  summary: &DailyCoolingSummary,
) -> Result<(), NativeDatabaseError> {
  let minutes = |band: &BandSummary| i64::from(band.sample_minutes);
  // Every real column here is nullable, so a NaN reading becomes the gap
  // SQLite would have stored rather than failing the day.
  let value = |reading: Option<f32>| {
    reading.and_then(|reading| sqlite_real_binding(f64::from(reading)))
  };
  transaction
    .connection()
    .execute(
      "INSERT INTO cooling_daily_summary (
         date,
         idle_cpu_temperature_avg, idle_cpu_temperature_max, idle_cpu_temperature_min, idle_sample_minutes,
         low_cpu_temperature_avg, low_cpu_temperature_max, low_cpu_temperature_min, low_sample_minutes,
         mid_cpu_temperature_avg, mid_cpu_temperature_max, mid_cpu_temperature_min, mid_sample_minutes,
         high_cpu_temperature_avg, high_cpu_temperature_max, high_cpu_temperature_min, high_sample_minutes,
         coverage_minutes,
         cpu_power_avg, cpu_power_max, cpu_power_min, power_sample_minutes
       )
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
       ON CONFLICT (date) DO UPDATE SET
         idle_cpu_temperature_avg = excluded.idle_cpu_temperature_avg,
         idle_cpu_temperature_max = excluded.idle_cpu_temperature_max,
         idle_cpu_temperature_min = excluded.idle_cpu_temperature_min,
         idle_sample_minutes = excluded.idle_sample_minutes,
         low_cpu_temperature_avg = excluded.low_cpu_temperature_avg,
         low_cpu_temperature_max = excluded.low_cpu_temperature_max,
         low_cpu_temperature_min = excluded.low_cpu_temperature_min,
         low_sample_minutes = excluded.low_sample_minutes,
         mid_cpu_temperature_avg = excluded.mid_cpu_temperature_avg,
         mid_cpu_temperature_max = excluded.mid_cpu_temperature_max,
         mid_cpu_temperature_min = excluded.mid_cpu_temperature_min,
         mid_sample_minutes = excluded.mid_sample_minutes,
         high_cpu_temperature_avg = excluded.high_cpu_temperature_avg,
         high_cpu_temperature_max = excluded.high_cpu_temperature_max,
         high_cpu_temperature_min = excluded.high_cpu_temperature_min,
         high_sample_minutes = excluded.high_sample_minutes,
         coverage_minutes = excluded.coverage_minutes,
         cpu_power_avg = excluded.cpu_power_avg,
         cpu_power_max = excluded.cpu_power_max,
         cpu_power_min = excluded.cpu_power_min,
         power_sample_minutes = excluded.power_sample_minutes",
      params![
        date_key(summary.date),
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
        i64::from(summary.coverage_minutes),
        value(summary.power.avg),
        value(summary.power.max),
        value(summary.power.min),
        i64::from(summary.power.sample_minutes),
      ],
    )
    .map_err(|error| {
      NativeDatabaseError::duckdb("upsert a daily cooling summary", error)
    })?;
  Ok(())
}

/// Upsert one summarized day on its own.
pub async fn upsert(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  summary: DailyCoolingSummary,
) -> Result<(), NativeDatabaseError> {
  database
    .request_write(cancellation, move |context| {
      context.with_transaction(|transaction| upsert_in(transaction, &summary))
    })
    .await
}

/// Delete rows older than `retention_days`, except those inside any of
/// `preserved_windows` - the pinned absolute idle baseline's calendar window,
/// exempt because deleting it would leave every baseline-side comparison
/// permanently empty while the pinned baseline still names that period as the
/// reference.
pub async fn delete_old_data(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  retention_days: u32,
  preserved_windows: &[(NaiveDate, NaiveDate)],
) -> Result<(), NativeDatabaseError> {
  let sql =
    preserving_delete_sql("cooling_daily_summary", "date", preserved_windows.len());
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
            NativeDatabaseError::duckdb("delete expired daily cooling summaries", error)
          })?;
        Ok(())
      })
    })
    .await
}
