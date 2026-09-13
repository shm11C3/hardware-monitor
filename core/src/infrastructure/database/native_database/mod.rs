//! Explicit native DuckDB access for a finalized, unselected database.
//!
//! SQLite remains authoritative. Nothing here is reached by the running
//! application: finalization builds a separate file, the owner opens it only
//! when a caller asks, and the per-family functions run beside - never instead
//! of - their SQLite counterparts until App lifecycle selection is implemented.

pub mod ambient_archive;
mod archive_sql;
mod binding;
mod cell;
pub mod cooling_baseline;
pub mod cooling_covariate_daily_summary;
pub mod cooling_daily_summary;
pub mod cooling_delta_baseline;
pub mod cooling_fan_daily_summary;
pub mod cooling_hourly_summary;
pub mod cooling_rollup;
pub mod cooling_thermal_delta_daily_summary;
pub mod data_archive;
mod epoch;
mod error;
pub mod fan_archive;
mod finalize;
pub mod gpu_archive;
mod paging;
mod preflight;
pub mod process_stats;
mod reconcile;
mod runtime;
mod schema;
mod selection;
mod series;
mod stored_text;
mod write_stamp;

use std::path::Path;

use duckdb::Connection;

pub use cooling_rollup::DayRollup;
pub use error::NativeDatabaseError;
pub use finalize::{
  NativeFinalizationReport, NativeTableReport, finalize_candidate_database,
};
pub use preflight::{
  ConversionSpaceObservation, ConversionSpacePlan, ConversionSpaceRequirement,
  conversion_space_requirement, plan_conversion_space,
};
pub use reconcile::{
  NativeReconciliationReport, NativeReconciliationTableReport, reconcile_native_database,
};
pub use runtime::{
  NativeCancellation, NativeConnectionContext, NativeDatabase, NativeDatabaseOptions,
  NativeTransactionContext,
};
pub use schema::{
  NativeIdentity, NativeIdentityMode, NativeSchemaDefinition, NativeTimestampColumn,
};
pub use selection::{
  AUTHORITY_MARKER_FILE_NAME, AuthorityFacts, AuthorityInconsistency, AuthorityMarker,
  AuthorityPaths, AuthorityRecovery, AuthorityState, MarkerFacts, NativeMetadataFacts,
  NativeState, VerifiedNativeDatabase, inspect_authority, observe_authority,
  repair_authority_marker, select_native_database,
};
pub use series::NativeSeriesWindow;

/// Point one DuckDB instance at its own spill directory and close it to the
/// filesystem afterwards.
///
/// DuckDB rejects `temp_directory` changes once external access is disabled, so
/// the owned spill path has to be set first. Independent instances must not
/// share a spill directory.
fn configure_spill(
  connection: &Connection,
  spill: &Path,
) -> Result<(), NativeDatabaseError> {
  let spill = spill.to_str().ok_or_else(|| NativeDatabaseError::Worker {
    message: format!("spill path is not valid UTF-8: {}", spill.display()),
  })?;
  connection
    .execute_batch(&format!(
      "SET temp_directory = '{}'; SET enable_external_access = false",
      spill.replace('\'', "''")
    ))
    .map_err(|error| NativeDatabaseError::duckdb("configure the spill directory", error))
}
