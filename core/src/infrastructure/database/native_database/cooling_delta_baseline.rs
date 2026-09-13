//! The `cooling_delta_baseline` single pinned row against a finalized native
//! database.
//!
//! Runs beside [`crate::infrastructure::database::cooling_delta_baseline`].
//! Its own table rather than columns on `cooling_baseline`, because two
//! baselines that establish at different times cannot share one write-once
//! row - and the same write-once rule applies here.

use chrono::{DateTime, Utc};
use duckdb::{OptionalExt, params};

use super::NativeDatabaseError;
use super::archive_sql::date_key;
use super::binding::required_real;
use super::process_stats::sqlite_timestamp_text;
use super::runtime::{NativeCancellation, NativeDatabase};
use super::stored_text;
use crate::persistence::cooling_delta_baseline::EstablishedDeltaBaseline;

const TABLE: &str = "cooling_delta_baseline";

pub async fn select_established_delta_baseline(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
) -> Result<Option<EstablishedDeltaBaseline>, NativeDatabaseError> {
  database
    .request_read(cancellation, move |context| {
      context.check_cancelled()?;
      let row: Option<(String, String, String, f64, i64)> = context
        .connection()
        .query_row(
          "SELECT source, window_start_date, window_end_date, delta_temperature_avg, sample_minutes
           FROM cooling_delta_baseline
           WHERE id = 1",
          [],
          |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
          },
        )
        .optional()
        .map_err(|error| {
          NativeDatabaseError::duckdb("read the pinned ΔT cooling baseline", error)
        })?;
      row
        .map(|(source, start, end, delta_temperature_avg, sample_minutes)| {
          Ok(EstablishedDeltaBaseline {
            source,
            delta_temperature_avg: delta_temperature_avg as f32,
            window_start_date: stored_text::date(TABLE, "window_start_date", &start)?,
            window_end_date: stored_text::date(TABLE, "window_end_date", &end)?,
            sample_minutes: stored_text::count(sample_minutes),
          })
        })
        .transpose()
    })
    .await
}

pub async fn insert_established_delta_baseline(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  baseline: &EstablishedDeltaBaseline,
  established_at: DateTime<Utc>,
) -> Result<(), NativeDatabaseError> {
  let source = baseline.source.clone();
  let window_start = date_key(baseline.window_start_date);
  let window_end = date_key(baseline.window_end_date);
  let delta_temperature_avg = required_real(
    TABLE,
    "delta_temperature_avg",
    f64::from(baseline.delta_temperature_avg),
  )?;
  let sample_minutes = i64::from(baseline.sample_minutes);
  let established_at = sqlite_timestamp_text(&established_at);
  database
    .request_write(cancellation, move |context| {
      context.with_transaction(|transaction| {
        transaction
          .connection()
          .execute(
            "INSERT INTO cooling_delta_baseline
               (id, source, window_start_date, window_end_date, delta_temperature_avg, sample_minutes, established_at)
             VALUES (1, ?, ?, ?, ?, ?, ?)
             ON CONFLICT DO NOTHING",
            params![
              source.as_str(),
              window_start.as_str(),
              window_end.as_str(),
              delta_temperature_avg,
              sample_minutes,
              established_at.as_str()
            ],
          )
          .map_err(|error| {
            NativeDatabaseError::duckdb("pin the ΔT cooling baseline", error)
          })?;
        Ok(())
      })
    })
    .await
}
