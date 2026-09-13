//! The `cooling_fan_daily_summary` projection against a finalized native
//! database.
//!
//! Runs beside [`crate::infrastructure::database::cooling_fan_daily_summary`].
//!
//! Keyed by `(date, source)` rather than by `date` alone: how many fans a
//! machine exposes is configuration-dependent, so each fan gets its own row and
//! a fan with no reading that day is simply absent.

use chrono::NaiveDate;
use duckdb::params;

use super::NativeDatabaseError;
use super::archive_sql::{date_key, local_retention_cutoff};
use super::binding::required_real;
use super::runtime::{NativeCancellation, NativeDatabase, NativeTransactionContext};
use super::stored_text;
use crate::persistence::cooling_fan_rollup::FanDailySummary;

const TABLE: &str = "cooling_fan_daily_summary";

/// `MAX(date)` - the fan projection's own catch-up cursor.
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
          "SELECT MAX(date) FROM cooling_fan_daily_summary",
          [],
          |row| row.get(0),
        )
        .map_err(|error| {
          NativeDatabaseError::duckdb("read the latest summarized fan day", error)
        })?;
      latest
        .map(|text| stored_text::date(TABLE, "date", &text))
        .transpose()
    })
    .await
}

/// Every summarized fan-day, oldest first and grouped by fan within a day.
pub async fn select_all_fan_daily_summaries(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
) -> Result<Vec<FanDailySummary>, NativeDatabaseError> {
  database
    .request_read(cancellation, move |context| {
      context.check_cancelled()?;
      let mut statement = context
        .connection()
        .prepare(
          "SELECT date, source, rpm_avg, rpm_max, rpm_min, sample_minutes
           FROM cooling_fan_daily_summary
           ORDER BY date ASC, source ASC",
        )
        .map_err(|error| {
          NativeDatabaseError::duckdb("prepare the fan daily summary query", error)
        })?;
      let rows = statement
        .query_map([], |row| {
          Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, f64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, i64>(5)?,
          ))
        })
        .map_err(|error| {
          NativeDatabaseError::duckdb("run the fan daily summary query", error)
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
          NativeDatabaseError::duckdb("decode a fan daily summary row", error)
        })?;
      rows
        .into_iter()
        .map(
          |(date, source, rpm_avg, rpm_max, rpm_min, sample_minutes)| {
            Ok(FanDailySummary {
              date: stored_text::date(TABLE, "date", &date)?,
              source,
              rpm_avg: rpm_avg as f32,
              rpm_max: stored_text::count(rpm_max),
              rpm_min: stored_text::count(rpm_min),
              sample_minutes: stored_text::count(sample_minutes),
            })
          },
        )
        .collect()
    })
    .await
}

/// Upsert one fan-day inside a caller's transaction.
pub(super) fn upsert_in(
  transaction: &NativeTransactionContext<'_, '_>,
  summary: &FanDailySummary,
) -> Result<(), NativeDatabaseError> {
  transaction
    .connection()
    .execute(
      "INSERT INTO cooling_fan_daily_summary
         (date, source, rpm_avg, rpm_max, rpm_min, sample_minutes)
       VALUES (?, ?, ?, ?, ?, ?)
       ON CONFLICT (date, source) DO UPDATE SET
         rpm_avg = excluded.rpm_avg,
         rpm_max = excluded.rpm_max,
         rpm_min = excluded.rpm_min,
         sample_minutes = excluded.sample_minutes",
      params![
        date_key(summary.date),
        summary.source.as_str(),
        required_real(TABLE, "rpm_avg", f64::from(summary.rpm_avg))?,
        i64::from(summary.rpm_max),
        i64::from(summary.rpm_min),
        i64::from(summary.sample_minutes),
      ],
    )
    .map_err(|error| NativeDatabaseError::duckdb("upsert a fan daily summary", error))?;
  Ok(())
}

/// Delete rows older than `retention_days`.
///
/// No preserved window: no pinned baseline is derived from this table, so
/// there is nothing here a comparison would keep reading past the cutoff.
pub async fn delete_old_data(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  retention_days: u32,
) -> Result<(), NativeDatabaseError> {
  let cutoff = local_retention_cutoff(retention_days);
  database
    .request_write(cancellation, move |context| {
      context.with_transaction(|transaction| {
        transaction
          .connection()
          .execute(
            "DELETE FROM cooling_fan_daily_summary WHERE date < ?",
            params![cutoff.as_str()],
          )
          .map_err(|error| {
            NativeDatabaseError::duckdb("delete expired fan daily summaries", error)
          })?;
        Ok(())
      })
    })
    .await
}
