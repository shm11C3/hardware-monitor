//! One rolled-up day's six cooling projections, written in a single native
//! transaction.
//!
//! The native form of
//! [`crate::persistence::cooling_rollup::persist_day_rollup_from_pool`], and
//! atomic for exactly the same reason: a committed daily row with its hourly
//! rows missing is a half-written state the catch-up cursor would have to
//! repair, and it cannot tell that case apart from a day that legitimately had
//! no pairs once the archive rows behind it age out. Failing the day as a whole
//! leaves the cursor unmoved, so the next pass simply retries it.
//!
//! Six tables, one boundary: `cooling_daily_summary`,
//! `cooling_hourly_summary`, `cooling_fan_daily_summary`,
//! `cooling_thermal_delta_daily_summary`, `cooling_covariate_daily_summary`
//! and `cooling_fan_covariate_daily_summary`. They are written in that order -
//! the order the SQLite transaction uses - so a failure part-way leaves the
//! same rollback either way and the two paths cannot diverge in what a
//! partially applied day would have looked like.

use super::NativeDatabaseError;
use super::runtime::{NativeCancellation, NativeDatabase};
use super::{
  cooling_covariate_daily_summary, cooling_daily_summary, cooling_fan_daily_summary,
  cooling_hourly_summary, cooling_thermal_delta_daily_summary,
};
use crate::persistence::cooling_covariate_rollup::CovariateDaySummary;
use crate::persistence::cooling_fan_rollup::FanDailySummary;
use crate::persistence::cooling_hourly_rollup::HourlyCoolingSummary;
use crate::persistence::cooling_rollup::DailyCoolingSummary;
use crate::persistence::cooling_thermal_delta_rollup::ThermalDeltaDailySummary;

/// Everything one rolled-up day writes, gathered so the six tables cross the
/// transaction boundary together rather than as five loose argument lists.
#[derive(Debug, Default)]
pub struct DayRollup {
  /// `None` for a day that produced no daily row at all - the other
  /// projections may still have rows, and the day still commits.
  pub summary: Option<DailyCoolingSummary>,
  pub hours: Vec<HourlyCoolingSummary>,
  pub fans: Vec<FanDailySummary>,
  pub thermal_deltas: Vec<ThermalDeltaDailySummary>,
  pub covariates: CovariateDaySummary,
}

/// Write one day's rollup projections in a single transaction.
pub async fn persist_day_rollup(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  rollup: DayRollup,
) -> Result<(), NativeDatabaseError> {
  database
    .request_write(cancellation, move |context| {
      context.with_transaction(|transaction| {
        if let Some(summary) = &rollup.summary {
          cooling_daily_summary::upsert_in(transaction, summary)?;
        }
        for hour in &rollup.hours {
          transaction.check_cancelled()?;
          cooling_hourly_summary::upsert_in(transaction, hour)?;
        }
        for fan in &rollup.fans {
          transaction.check_cancelled()?;
          cooling_fan_daily_summary::upsert_in(transaction, fan)?;
        }
        for thermal_delta in &rollup.thermal_deltas {
          transaction.check_cancelled()?;
          cooling_thermal_delta_daily_summary::upsert_in(transaction, thermal_delta)?;
        }
        for covariate in &rollup.covariates.bands {
          transaction.check_cancelled()?;
          cooling_covariate_daily_summary::upsert_in(transaction, covariate)?;
        }
        for covariate in &rollup.covariates.fans {
          transaction.check_cancelled()?;
          cooling_covariate_daily_summary::upsert_fan_in(transaction, covariate)?;
        }
        Ok(())
      })
    })
    .await
}
