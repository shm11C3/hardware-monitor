use duckdb::{AccessMode, Config, Connection};

use super::NativeDatabaseError;

/// Keep new native files at the compatibility level shipped by the current
/// bundled DuckDB build. A crate upgrade must change this deliberately after a
/// format review.
pub(super) const NATIVE_STORAGE_COMPATIBILITY_VERSION: &str = "v0.10.2";
/// DuckDB reports the compatibility level used by storage version 64 with this
/// tag. The tag is read from the opened database rather than inferred from the
/// crate version.
pub(super) const NATIVE_STORAGE_VERSION_TAG: &str = "v1.0.0+";

pub(super) fn native_config(
  access_mode: AccessMode,
  pin_storage_version: bool,
) -> Result<Config, NativeDatabaseError> {
  Config::default()
    .access_mode(access_mode)
    .and_then(|config| config.threads(2))
    .and_then(|config| config.max_memory("128MB"))
    .and_then(|config| config.enable_autoload_extension(false))
    .and_then(|config| {
      if pin_storage_version {
        config.with(
          "storage_compatibility_version",
          NATIVE_STORAGE_COMPATIBILITY_VERSION,
        )
      } else {
        Ok(config)
      }
    })
    .map_err(|error| NativeDatabaseError::duckdb("configure a native database", error))
}

/// Read the storage compatibility tag DuckDB attached to the opened file.
pub(super) fn engine_storage_version(
  connection: &Connection,
) -> Result<String, NativeDatabaseError> {
  let version: Option<String> = connection
    .query_row(
      "SELECT tags['storage_version'] FROM duckdb_databases() \
       WHERE database_name = current_database()",
      [],
      |row| row.get(0),
    )
    .map_err(|error| {
      NativeDatabaseError::duckdb("read the engine storage version", error)
    })?;
  version.ok_or_else(|| NativeDatabaseError::Verification {
    message: "the engine did not report a storage version for the native database"
      .to_owned(),
  })
}

pub(super) fn verify_storage_version(
  connection: &Connection,
  recorded: &str,
) -> Result<(), NativeDatabaseError> {
  let actual = engine_storage_version(connection)?;
  if actual != recorded {
    return Err(NativeDatabaseError::StorageVersionMismatch {
      expected: recorded.to_owned(),
      actual,
    });
  }
  Ok(())
}
