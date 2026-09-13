//! The `cooling_baseline` single pinned row against a finalized native
//! database.
//!
//! Runs beside [`crate::infrastructure::database::cooling_baseline`].
//!
//! Establishment is write-once. SQLite spells that `INSERT OR IGNORE` against
//! the fixed `id = 1`; DuckDB spells it `ON CONFLICT DO NOTHING` against the
//! same primary key, and the finalized schema carries the `CHECK (id = 1)` that
//! keeps the table one row wide. Replacing the pinned value is exactly the
//! drift this table exists to prevent, so a second establishment must change
//! nothing - not even `established_at`.

use chrono::{DateTime, Utc};
use duckdb::{OptionalExt, params};

use super::NativeDatabaseError;
use super::archive_sql::date_key;
use super::binding::required_real;
use super::process_stats::sqlite_timestamp_text;
use super::runtime::{NativeCancellation, NativeDatabase};
use super::stored_text;
use crate::persistence::cooling_baseline::EstablishedBaseline;

const TABLE: &str = "cooling_baseline";

pub async fn select_established_baseline(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
) -> Result<Option<EstablishedBaseline>, NativeDatabaseError> {
  database
    .request_read(cancellation, move |context| {
      context.check_cancelled()?;
      let row: Option<(String, String, f64, i64)> = context
        .connection()
        .query_row(
          "SELECT window_start_date, window_end_date, idle_temperature_avg, sample_minutes
           FROM cooling_baseline
           WHERE id = 1",
          [],
          |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(|error| {
          NativeDatabaseError::duckdb("read the pinned cooling baseline", error)
        })?;
      row
        .map(|(start, end, idle_temperature_avg, sample_minutes)| {
          Ok(EstablishedBaseline {
            idle_temperature_avg: idle_temperature_avg as f32,
            window_start_date: stored_text::date(TABLE, "window_start_date", &start)?,
            window_end_date: stored_text::date(TABLE, "window_end_date", &end)?,
            sample_minutes: stored_text::count(sample_minutes),
          })
        })
        .transpose()
    })
    .await
}

pub async fn insert_established_baseline(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  baseline: &EstablishedBaseline,
  established_at: DateTime<Utc>,
) -> Result<(), NativeDatabaseError> {
  let window_start = date_key(baseline.window_start_date);
  let window_end = date_key(baseline.window_end_date);
  let idle_temperature_avg = required_real(
    TABLE,
    "idle_temperature_avg",
    f64::from(baseline.idle_temperature_avg),
  )?;
  let sample_minutes = i64::from(baseline.sample_minutes);
  let established_at = sqlite_timestamp_text(&established_at);
  database
    .request_write(cancellation, move |context| {
      context.with_transaction(|transaction| {
        transaction
          .connection()
          .execute(
            "INSERT INTO cooling_baseline
               (id, window_start_date, window_end_date, idle_temperature_avg, sample_minutes, established_at)
             VALUES (1, ?, ?, ?, ?, ?)
             ON CONFLICT DO NOTHING",
            params![
              window_start.as_str(),
              window_end.as_str(),
              idle_temperature_avg,
              sample_minutes,
              established_at.as_str()
            ],
          )
          .map_err(|error| {
            NativeDatabaseError::duckdb("pin the cooling baseline", error)
          })?;
        Ok(())
      })
    })
    .await
}
