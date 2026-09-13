//! The `cooling_covariate_daily_summary` and
//! `cooling_fan_covariate_daily_summary` projections against a finalized
//! native database.
//!
//! Runs beside
//! [`crate::infrastructure::database::cooling_covariate_daily_summary`].
//!
//! One module for both tables, as in SQLite: they are two shapes of one
//! projection, folded from one read and written in one transaction, and
//! nothing reads one without the other. The ambient source is on every row
//! because which sensor the Thermal Delta was measured against is part of the
//! fit, so a sensor change can never blend two placements into one row.

use chrono::{DateTime, NaiveDate, Utc};
use duckdb::params;

use super::NativeDatabaseError;
use super::archive_sql::{date_key, local_retention_cutoff, preserving_delete_sql};
use super::binding::{required_real, sqlite_real_binding};
use super::cooling_thermal_delta_daily_summary::pairable_ambient_cursor;
use super::runtime::{NativeCancellation, NativeDatabase, NativeTransactionContext};
use super::stored_text;
use crate::persistence::cooling_covariate_rollup::{
  CovariateDailySummary, FanCovariateDailySummary, PairedFitStatistics,
};

const TABLE: &str = "cooling_covariate_daily_summary";
const FAN_TABLE: &str = "cooling_fan_covariate_daily_summary";

/// The hardware-side clause that makes a paired minute classifiable.
const CLASSIFIABLE_PREDICATE: &str = "AND DATA_ARCHIVE.cpu_avg IS NOT NULL";

/// `MAX(date)` - the co-variate projection's own catch-up cursor.
///
/// The fan table has no cursor of its own: a fan row needs a paired
/// classifiable minute to sit beside, so it never exists on a day the band
/// table has no row for.
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
          "SELECT MAX(date) FROM cooling_covariate_daily_summary",
          [],
          |row| row.get(0),
        )
        .map_err(|error| {
          NativeDatabaseError::duckdb("read the latest summarized co-variate day", error)
        })?;
      latest
        .map(|text| stored_text::date(TABLE, "date", &text))
        .transpose()
    })
    .await
}

/// The most recent ambient archive timestamp strictly before `before` whose
/// minute also has a `DATA_ARCHIVE` row carrying a CPU usage reading.
///
/// The ΔT cursor narrowed by one more predicate, because this rollup's row
/// gate is one step stricter: a paired minute with no usage reading has no
/// band to be filed under and yields no row here, so counting it as evidence
/// of a missed day would send the catch-up chasing a day it can never fill.
pub async fn max_classifiable_pairable_ambient_archive_timestamp_before(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  before: &DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, NativeDatabaseError> {
  pairable_ambient_cursor(
    database,
    cancellation,
    before,
    CLASSIFIABLE_PREDICATE,
    "read the latest classifiable pairable ambient stamp",
  )
  .await
}

/// Every summarized source-band-day, oldest first and grouped by source then
/// band within a day.
pub async fn select_all_covariate_daily_summaries(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
) -> Result<Vec<CovariateDailySummary>, NativeDatabaseError> {
  database
    .request_read(cancellation, move |context| {
      context.check_cancelled()?;
      let mut statement = context
        .connection()
        .prepare(
          "SELECT date, source, band, sample_minutes, band_share, ambient_temperature_median,
             delta_minutes, delta_temperature_median, power_minutes, cpu_power_median,
             power_fit_n, power_fit_sum_x, power_fit_sum_y, power_fit_sum_xy, power_fit_sum_xx, power_fit_sum_yy
           FROM cooling_covariate_daily_summary
           ORDER BY date ASC, source ASC, band ASC",
        )
        .map_err(|error| {
          NativeDatabaseError::duckdb("prepare the co-variate daily summary query", error)
        })?;
      let rows = statement
        .query_map([], |row| {
          let mut fit = [0f64; 5];
          for (slot, column) in fit.iter_mut().zip([11, 12, 13, 14, 15]) {
            *slot = row.get::<_, f64>(column)?;
          }
          Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            [
              row.get::<_, i64>(3)?,
              row.get::<_, i64>(6)?,
              row.get::<_, i64>(8)?,
              row.get::<_, i64>(10)?,
            ],
            row.get::<_, f64>(4)?,
            row.get::<_, f64>(5)?,
            row.get::<_, Option<f64>>(7)?,
            row.get::<_, Option<f64>>(9)?,
            fit,
          ))
        })
        .map_err(|error| {
          NativeDatabaseError::duckdb("run the co-variate daily summary query", error)
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
          NativeDatabaseError::duckdb("decode a co-variate daily summary row", error)
        })?;
      rows
        .into_iter()
        .map(
          |(
            date,
            source,
            band,
            counts,
            band_share,
            ambient_temperature_median,
            delta_temperature_median,
            cpu_power_median,
            fit,
          )| {
            Ok(CovariateDailySummary {
              date: stored_text::date(TABLE, "date", &date)?,
              source,
              band: stored_text::band(TABLE, &band)?,
              sample_minutes: stored_text::count(counts[0]),
              band_share: band_share as f32,
              ambient_temperature_median: ambient_temperature_median as f32,
              delta_minutes: stored_text::count(counts[1]),
              delta_temperature_median: delta_temperature_median
                .map(|value| value as f32),
              power_minutes: stored_text::count(counts[2]),
              cpu_power_median: cpu_power_median.map(|value| value as f32),
              delta_per_watt: PairedFitStatistics {
                n: stored_text::count(counts[3]),
                sum_x: fit[0],
                sum_y: fit[1],
                sum_xy: fit[2],
                sum_xx: fit[3],
                sum_yy: fit[4],
              },
            })
          },
        )
        .collect()
    })
    .await
}

/// Every summarized fan row, oldest first and grouped by source, fan and band
/// within a day.
pub async fn select_all_fan_covariate_daily_summaries(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
) -> Result<Vec<FanCovariateDailySummary>, NativeDatabaseError> {
  database
    .request_read(cancellation, move |context| {
      context.check_cancelled()?;
      let mut statement = context
        .connection()
        .prepare(
          "SELECT date, source, fan_source, band, rpm_minutes, rpm_median,
             fit_n, fit_sum_x, fit_sum_y, fit_sum_xy, fit_sum_xx, fit_sum_yy
           FROM cooling_fan_covariate_daily_summary
           ORDER BY date ASC, source ASC, fan_source ASC, band ASC",
        )
        .map_err(|error| {
          NativeDatabaseError::duckdb(
            "prepare the fan co-variate daily summary query",
            error,
          )
        })?;
      let rows = statement
        .query_map([], |row| {
          let mut fit = [0f64; 5];
          for (slot, column) in fit.iter_mut().zip([7, 8, 9, 10, 11]) {
            *slot = row.get::<_, f64>(column)?;
          }
          Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, f64>(5)?,
            row.get::<_, i64>(6)?,
            fit,
          ))
        })
        .map_err(|error| {
          NativeDatabaseError::duckdb("run the fan co-variate daily summary query", error)
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
          NativeDatabaseError::duckdb("decode a fan co-variate daily summary row", error)
        })?;
      rows
        .into_iter()
        .map(
          |(date, source, fan_source, band, rpm_minutes, rpm_median, fit_n, fit)| {
            Ok(FanCovariateDailySummary {
              date: stored_text::date(FAN_TABLE, "date", &date)?,
              source,
              fan_source,
              band: stored_text::band(FAN_TABLE, &band)?,
              rpm_minutes: stored_text::count(rpm_minutes),
              rpm_median: rpm_median as f32,
              delta_per_rpm: PairedFitStatistics {
                n: stored_text::count(fit_n),
                sum_x: fit[0],
                sum_y: fit[1],
                sum_xy: fit[2],
                sum_xx: fit[3],
                sum_yy: fit[4],
              },
            })
          },
        )
        .collect()
    })
    .await
}

/// Upsert one source-band-day inside a caller's transaction.
pub(super) fn upsert_in(
  transaction: &NativeTransactionContext<'_, '_>,
  summary: &CovariateDailySummary,
) -> Result<(), NativeDatabaseError> {
  let fit = &summary.delta_per_watt;
  transaction
    .connection()
    .execute(
      "INSERT INTO cooling_covariate_daily_summary (
         date, source, band, sample_minutes, band_share, ambient_temperature_median,
         delta_minutes, delta_temperature_median, power_minutes, cpu_power_median,
         power_fit_n, power_fit_sum_x, power_fit_sum_y, power_fit_sum_xy, power_fit_sum_xx, power_fit_sum_yy
       )
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
       ON CONFLICT (date, source, band) DO UPDATE SET
         sample_minutes = excluded.sample_minutes,
         band_share = excluded.band_share,
         ambient_temperature_median = excluded.ambient_temperature_median,
         delta_minutes = excluded.delta_minutes,
         delta_temperature_median = excluded.delta_temperature_median,
         power_minutes = excluded.power_minutes,
         cpu_power_median = excluded.cpu_power_median,
         power_fit_n = excluded.power_fit_n,
         power_fit_sum_x = excluded.power_fit_sum_x,
         power_fit_sum_y = excluded.power_fit_sum_y,
         power_fit_sum_xy = excluded.power_fit_sum_xy,
         power_fit_sum_xx = excluded.power_fit_sum_xx,
         power_fit_sum_yy = excluded.power_fit_sum_yy",
      params![
        date_key(summary.date),
        summary.source.as_str(),
        summary.band.column_key(),
        i64::from(summary.sample_minutes),
        required_real(TABLE, "band_share", f64::from(summary.band_share))?,
        required_real(
          TABLE,
          "ambient_temperature_median",
          f64::from(summary.ambient_temperature_median),
        )?,
        i64::from(summary.delta_minutes),
        summary
          .delta_temperature_median
          .and_then(|reading| sqlite_real_binding(f64::from(reading))),
        i64::from(summary.power_minutes),
        summary
          .cpu_power_median
          .and_then(|reading| sqlite_real_binding(f64::from(reading))),
        i64::from(fit.n),
        required_real(TABLE, "power_fit_sum_x", fit.sum_x)?,
        required_real(TABLE, "power_fit_sum_y", fit.sum_y)?,
        required_real(TABLE, "power_fit_sum_xy", fit.sum_xy)?,
        required_real(TABLE, "power_fit_sum_xx", fit.sum_xx)?,
        required_real(TABLE, "power_fit_sum_yy", fit.sum_yy)?,
      ],
    )
    .map_err(|error| {
      NativeDatabaseError::duckdb("upsert a co-variate daily summary", error)
    })?;
  Ok(())
}

/// Upsert one fan row inside a caller's transaction.
pub(super) fn upsert_fan_in(
  transaction: &NativeTransactionContext<'_, '_>,
  summary: &FanCovariateDailySummary,
) -> Result<(), NativeDatabaseError> {
  let fit = &summary.delta_per_rpm;
  transaction
    .connection()
    .execute(
      "INSERT INTO cooling_fan_covariate_daily_summary (
         date, source, fan_source, band, rpm_minutes, rpm_median,
         fit_n, fit_sum_x, fit_sum_y, fit_sum_xy, fit_sum_xx, fit_sum_yy
       )
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
       ON CONFLICT (date, source, fan_source, band) DO UPDATE SET
         rpm_minutes = excluded.rpm_minutes,
         rpm_median = excluded.rpm_median,
         fit_n = excluded.fit_n,
         fit_sum_x = excluded.fit_sum_x,
         fit_sum_y = excluded.fit_sum_y,
         fit_sum_xy = excluded.fit_sum_xy,
         fit_sum_xx = excluded.fit_sum_xx,
         fit_sum_yy = excluded.fit_sum_yy",
      params![
        date_key(summary.date),
        summary.source.as_str(),
        summary.fan_source.as_str(),
        summary.band.column_key(),
        i64::from(summary.rpm_minutes),
        required_real(FAN_TABLE, "rpm_median", f64::from(summary.rpm_median))?,
        i64::from(fit.n),
        required_real(FAN_TABLE, "fit_sum_x", fit.sum_x)?,
        required_real(FAN_TABLE, "fit_sum_y", fit.sum_y)?,
        required_real(FAN_TABLE, "fit_sum_xy", fit.sum_xy)?,
        required_real(FAN_TABLE, "fit_sum_xx", fit.sum_xx)?,
        required_real(FAN_TABLE, "fit_sum_yy", fit.sum_yy)?,
      ],
    )
    .map_err(|error| {
      NativeDatabaseError::duckdb("upsert a fan co-variate daily summary", error)
    })?;
  Ok(())
}

/// Delete rows of both tables older than `retention_days`, except those inside
/// any of `preserved_windows` - the pinned ΔT baseline's calendar window, from
/// which the co-variate comparison reads its baseline side.
///
/// Both tables go in one transaction: they are two shapes of one projection,
/// and a cutoff applied to one but not the other would leave a band row whose
/// fan rows had aged out from under it.
pub async fn delete_old_data(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  retention_days: u32,
  preserved_windows: &[(NaiveDate, NaiveDate)],
) -> Result<(), NativeDatabaseError> {
  let mut bounds = vec![local_retention_cutoff(retention_days)];
  for (start, end) in preserved_windows {
    bounds.push(date_key(*start));
    bounds.push(date_key(*end));
  }
  let statements = [TABLE, FAN_TABLE]
    .map(|table| preserving_delete_sql(table, "date", preserved_windows.len()));
  database
    .request_write(cancellation, move |context| {
      context.with_transaction(|transaction| {
        for sql in &statements {
          transaction.check_cancelled()?;
          transaction
            .connection()
            .execute(sql, duckdb::params_from_iter(bounds.iter()))
            .map_err(|error| {
              NativeDatabaseError::duckdb(
                "delete expired co-variate daily summaries",
                error,
              )
            })?;
        }
        Ok(())
      })
    })
    .await
}
