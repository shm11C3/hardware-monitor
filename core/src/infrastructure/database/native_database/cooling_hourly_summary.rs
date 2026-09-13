//! The `cooling_hourly_summary` projection against a finalized native
//! database.
//!
//! Runs beside [`crate::infrastructure::database::cooling_hourly_summary`].
//!
//! `hour_start` is the local wall-clock hour string `cooling_hourly_rollup`
//! formats. Every comparison here is a byte-wise text comparison against it -
//! the same rule in both engines - and a row whose `hour_start` is not in the
//! stored format is dropped rather than failing the read, exactly as the
//! sqlx reader's `filter_map` does: one hand-edited row must not take the
//! Explorer's whole query with it.

use chrono::NaiveDate;
use duckdb::params;

use super::NativeDatabaseError;
use super::archive_sql::{date_key, local_retention_cutoff, preserving_delete_sql};
use super::binding::sqlite_real_binding;
use super::runtime::{NativeCancellation, NativeDatabase, NativeTransactionContext};
use super::stored_text;
use crate::persistence::cooling_hourly_rollup::{
  HourlyCoolingSummary, format_hour_start, parse_hour_start,
};

/// Every hourly row whose local day falls in `[start_date, end_date]`
/// (inclusive), oldest first.
///
/// Bounded rather than a whole-table read, as in SQLite: this table holds 24x
/// the rows of the daily projection for the same retention window, and the
/// Explorer only ever looks at two bounded windows.
pub async fn select_hours_in_date_range(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  start_date: NaiveDate,
  end_date: NaiveDate,
) -> Result<Vec<HourlyCoolingSummary>, NativeDatabaseError> {
  // `>= "YYYY-MM-DD"` includes that day's 00:00 hour, and `<` the day after
  // `end_date` includes its 23:00 hour - the half-open upper bound avoids a
  // literal that would depend on the hour format's exact width.
  let start = date_key(start_date);
  let end = date_key(end_date + chrono::Duration::days(1));
  database
    .request_read(cancellation, move |context| {
      context.check_cancelled()?;
      let mut statement = context
        .connection()
        .prepare(
          "SELECT hour_start, cpu_usage_avg, cpu_temperature_avg, sample_minutes
           FROM cooling_hourly_summary
           WHERE hour_start >= ? AND hour_start < ?
           ORDER BY hour_start ASC",
        )
        .map_err(|error| {
          NativeDatabaseError::duckdb("prepare the hourly cooling query", error)
        })?;
      let rows = statement
        .query_map(params![start.as_str(), end.as_str()], |row| {
          Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<f64>>(1)?,
            row.get::<_, Option<f64>>(2)?,
            row.get::<_, i64>(3)?,
          ))
        })
        .map_err(|error| {
          NativeDatabaseError::duckdb("run the hourly cooling query", error)
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
          NativeDatabaseError::duckdb("decode an hourly cooling row", error)
        })?;
      Ok(
        rows
          .into_iter()
          .filter_map(
            |(hour_start, cpu_usage_avg, cpu_temperature_avg, minutes)| {
              Some(HourlyCoolingSummary {
                hour_start: parse_hour_start(&hour_start)?,
                cpu_usage_avg: cpu_usage_avg.map(|value| value as f32),
                cpu_temperature_avg: cpu_temperature_avg.map(|value| value as f32),
                sample_minutes: stored_text::count(minutes),
              })
            },
          )
          .collect(),
      )
    })
    .await
}

/// The local day of the most recent hourly row, or `None` when the table is
/// empty - which is how the catch-up tells "hourly is behind" apart from
/// "hourly has never run".
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
          "SELECT MAX(hour_start) FROM cooling_hourly_summary",
          [],
          |row| row.get(0),
        )
        .map_err(|error| {
          NativeDatabaseError::duckdb("read the latest summarized hour", error)
        })?;
      // An unparseable `MAX` reads as "no summarized day", the same answer
      // the sqlx reader's `and_then` gives.
      Ok(latest.and_then(|raw| parse_hour_start(&raw).map(|hour| hour.date())))
    })
    .await
}

/// Upsert one summarized hour inside a caller's transaction.
pub(super) fn upsert_in(
  transaction: &NativeTransactionContext<'_, '_>,
  summary: &HourlyCoolingSummary,
) -> Result<(), NativeDatabaseError> {
  transaction
    .connection()
    .execute(
      "INSERT INTO cooling_hourly_summary
         (hour_start, cpu_usage_avg, cpu_temperature_avg, sample_minutes)
       VALUES (?, ?, ?, ?)
       ON CONFLICT (hour_start) DO UPDATE SET
         cpu_usage_avg = excluded.cpu_usage_avg,
         cpu_temperature_avg = excluded.cpu_temperature_avg,
         sample_minutes = excluded.sample_minutes",
      params![
        format_hour_start(summary.hour_start),
        summary
          .cpu_usage_avg
          .and_then(|reading| sqlite_real_binding(f64::from(reading))),
        summary
          .cpu_temperature_avg
          .and_then(|reading| sqlite_real_binding(f64::from(reading))),
        i64::from(summary.sample_minutes),
      ],
    )
    .map_err(|error| {
      NativeDatabaseError::duckdb("upsert an hourly cooling summary", error)
    })?;
  Ok(())
}

/// Upsert one summarized hour on its own.
pub async fn upsert(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  summary: HourlyCoolingSummary,
) -> Result<(), NativeDatabaseError> {
  database
    .request_write(cancellation, move |context| {
      context.with_transaction(|transaction| upsert_in(transaction, &summary))
    })
    .await
}

/// Delete rows older than `retention_days`, except those inside any of
/// `preserved_windows`.
///
/// The same local-date cutoff the daily projection uses, so both tables age
/// out on exactly the same boundary. Comparing an hour key against a bare date
/// string is correct because every hour of a day sorts after that day's date
/// string; the exemption's upper bound is suffixed past any hour a day can
/// carry so `<= ?` still covers that day's 23:00 row.
pub async fn delete_old_data(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  retention_days: u32,
  preserved_windows: &[(NaiveDate, NaiveDate)],
) -> Result<(), NativeDatabaseError> {
  let sql = preserving_delete_sql(
    "cooling_hourly_summary",
    "hour_start",
    preserved_windows.len(),
  );
  let mut bounds = vec![local_retention_cutoff(retention_days)];
  for (start, end) in preserved_windows {
    bounds.push(date_key(*start));
    bounds.push(format!("{} 24", date_key(*end)));
  }
  database
    .request_write(cancellation, move |context| {
      context.with_transaction(|transaction| {
        transaction
          .connection()
          .execute(&sql, duckdb::params_from_iter(bounds.iter()))
          .map_err(|error| {
            NativeDatabaseError::duckdb("delete expired hourly cooling summaries", error)
          })?;
        Ok(())
      })
    })
    .await
}
