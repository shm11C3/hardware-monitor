//! The Storage Health family against a finalized native database.
//!
//! Runs *beside* [`crate::infrastructure::database::storage_health`], never
//! instead of it. SQLite stays authoritative and keeps serving the
//! application; choosing between the two backends is a separate change.
//! Keeping both callable is what lets a test put one fixture through each path
//! and compare the answers bit for bit.
//!
//! Four differences from the SQLite module, each measured rather than assumed:
//!
//! - **`COLLATE NOCASE` is not spelled `COLLATE NOCASE`.** DuckDB has a
//!   collation by that name, but it is not SQLite's. SQLite's `NOCASE` folds
//!   only the 26 ASCII letters and then compares bytes; DuckDB's folds case
//!   across Unicode. Measured on one list of device names, the two orders
//!   disagree: SQLite answers `Ábc, Älpha, ábc, älpha` and DuckDB's `NOCASE`
//!   answers `Ábc, ábc, Älpha, älpha`. A device whose `display_name` the OS
//!   reports with a non-ASCII letter would therefore appear in a different
//!   place in the Storage Health list depending on which engine answered, so
//!   the native query orders by [`sqlite_nocase_key`] - SQLite's own folding,
//!   spelled as a `translate` over the ASCII letters - instead. Ties under
//!   that key are unordered in both engines, exactly as they were before.
//! - **A missing table is not tolerated.** The SQLite reader returns an empty
//!   result for `no such table`, because a database that predates the
//!   storage-health migration genuinely has no records yet. A finalized native
//!   database cannot be in that state: finalization creates every table in
//!   `get_native_schema()` and refuses a candidate that does not match, so
//!   `no such table` here would mean the file is not the schema it claims to
//!   be. That is a defect, and it surfaces as the DuckDB error it is rather
//!   than as "this disk has no history".
//! - **No stored text is decoded.** `date` and `collected_at` reach
//!   [`StorageHealthRecord`] as the stored `String`, exactly as the sqlx reader
//!   hands them over, so there is no date grammar to transcribe here and
//!   [`super::stored_text`] has nothing to do. The two enum columns are mapped
//!   by the SQLite module's own [`parse_health_status`]/[`parse_warning_level`],
//!   reused rather than restated so the two backends cannot drift apart about
//!   what `"critical"` means.
//! - **Retention returns the row count.** The SQLite function discards it; the
//!   native one reports it the way every other native family does, so a
//!   differential test can compare what each engine actually took.
//!
//! One thing that is deliberately *not* a difference: the daily-record `id`.
//! `storage_health_daily_records.id` is `INTEGER PRIMARY KEY AUTOINCREMENT` in
//! SQLite, and an upsert that lands on `DO UPDATE` still consumes a sequence
//! value there - measured, with `sqlite_sequence` advancing 1 → 2 on a
//! conflicting upsert and the next inserted row receiving id 3, not 2. So the
//! native writer allocates through [`super::runtime::NativeTransactionContext::next_id`]
//! on every record, conflict or not, and burns the same ids SQLite burns.

use duckdb::params;

use super::NativeDatabaseError;
use super::archive_sql::local_retention_cutoff;
use super::binding::sqlite_real_binding;
use super::runtime::{NativeCancellation, NativeDatabase, NativeTransactionContext};
use crate::infrastructure::database::storage_health::{
  from_i64, parse_health_status, parse_warning_level, to_i64,
};
use crate::models::hardware::{
  StorageDeviceRecord, StorageHealthRecord, StorageHealthRecordDraft,
};

const RECORDS_TABLE: &str = "storage_health_daily_records";

/// SQLite's `NOCASE` collation, as an ordering key DuckDB can compute.
///
/// `sqlite3StrNICmp` maps each byte through SQLite's upper-to-lower table -
/// identity above 0x7F - and compares the results, so folding `A-Z` to `a-z`
/// and letting DuckDB's byte-wise VARCHAR comparison do the rest reproduces it.
/// The direction is load-bearing: folding to upper case instead would sort
/// `_` after every letter, and SQLite sorts it before `a`.
fn sqlite_nocase_key(expression: &str) -> String {
  format!(
    "translate({expression}, 'ABCDEFGHIJKLMNOPQRSTUVWXYZ', 'abcdefghijklmnopqrstuvwxyz')"
  )
}

/// Upsert the devices and then their daily records, in one transaction - the
/// same boundary as the SQLite writer, so a failed record leaves no device
/// row behind either.
pub async fn insert_daily_records(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  devices: Vec<StorageDeviceRecord>,
  records: Vec<StorageHealthRecordDraft>,
) -> Result<(), NativeDatabaseError> {
  database
    .request_write(cancellation, move |context| {
      context.with_transaction(|transaction| insert_in(transaction, &devices, &records))
    })
    .await
}

/// [`insert_daily_records`] plus the active-device reconciliation, in the same
/// single transaction the SQLite writer uses.
///
/// The guard is the SQLite one verbatim: with no active ids *and* nothing to
/// write there is no transaction to open, and an empty `active_device_ids`
/// beside real rows writes the rows without clearing any flag. Deactivating
/// every device because an enumeration came back empty is the "temporary
/// enumeration failure is uncertainty" rule, and it lives in the guard.
pub async fn refresh_daily_records(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  active_device_ids: Vec<String>,
  devices: Vec<StorageDeviceRecord>,
  records: Vec<StorageHealthRecordDraft>,
) -> Result<(), NativeDatabaseError> {
  if active_device_ids.is_empty() && (devices.is_empty() || records.is_empty()) {
    return Ok(());
  }
  database
    .request_write(cancellation, move |context| {
      context.with_transaction(|transaction| {
        insert_in(transaction, &devices, &records)?;
        update_active_devices_in(transaction, &active_device_ids)
      })
    })
    .await
}

/// Delete daily records older than the Retention Period, returning how many
/// went.
///
/// `storage_devices` is deliberately untouched, exactly as in SQLite: a disk
/// whose history has aged out is still a disk this machine has seen, and its
/// identity row is what a returning device is recognised by.
///
/// The cutoff is a local calendar day rendered `%Y-%m-%d`, the same shared
/// [`local_retention_cutoff`] the cooling projections delete against, and the
/// comparison is byte-wise in both engines: SQLite applies the TEXT column's
/// affinity to the bound parameter, which is not a well-formed number and so
/// stays TEXT under the BINARY collation, and DuckDB compares VARCHAR by the
/// same byte order.
pub async fn delete_old_data(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  retention_days: u32,
) -> Result<u64, NativeDatabaseError> {
  let cutoff = local_retention_cutoff(retention_days);
  database
    .request_write(cancellation, move |context| {
      context.with_transaction(|transaction| {
        let deleted = transaction
          .connection()
          .execute(
            "DELETE FROM storage_health_daily_records WHERE date < ?",
            params![cutoff.as_str()],
          )
          .map_err(|error| {
            NativeDatabaseError::duckdb("delete expired storage health records", error)
          })?;
        Ok(deleted as u64)
      })
    })
    .await
}

/// The most recent health record per active device.
///
/// The SQLite query transcribed clause for clause: the per-device `MAX(date)`
/// sub-select, the inner join that keeps only the row matching it, the left
/// join that supplies the device labels, `COALESCE(d.is_active, 1) = 1` so a
/// record whose device row has not been written yet is still shown, and the
/// severity-then-name ordering. Only the collation is spelled differently, for
/// the reason in the module header.
///
/// A device with two records sharing its maximum `date` cannot exist - the
/// `UNIQUE(device_id, date)` constraint that the upsert's conflict target
/// names forbids it - so the inner join returns exactly one row per device.
pub async fn latest_records(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
) -> Result<Vec<StorageHealthRecord>, NativeDatabaseError> {
  let display_name = "COALESCE(d.display_name, s.device_id)";
  let sql = format!(
    "SELECT
       s.device_id,
       {display_name} AS display_name,
       d.model,
       d.protocol,
       d.capacity_bytes,
       s.date,
       s.health_status,
       s.warning_level,
       s.warning_reasons,
       s.temperature_celsius,
       s.power_on_hours,
       s.percentage_used,
       s.available_spare_percent,
       s.reallocated_sector_count,
       s.current_pending_sector_count,
       s.offline_uncorrectable_count,
       s.media_errors,
       s.error_log_entries,
       s.unsafe_shutdown_count,
       s.collected_at
     FROM storage_health_daily_records s
     INNER JOIN (
       SELECT device_id, MAX(date) AS date
       FROM storage_health_daily_records
       GROUP BY device_id
     ) latest
       ON latest.device_id = s.device_id
       AND latest.date = s.date
     LEFT JOIN storage_devices d ON d.id = s.device_id
     WHERE COALESCE(d.is_active, 1) = 1
     ORDER BY
       CASE s.warning_level
         WHEN 'critical' THEN 0
         WHEN 'warning' THEN 1
         WHEN 'unknown' THEN 2
         ELSE 3
       END,
       {}",
    sqlite_nocase_key(display_name)
  );
  database
    .request_read(cancellation, move |context| {
      context.check_cancelled()?;
      let mut statement = context.connection().prepare(&sql).map_err(|error| {
        NativeDatabaseError::duckdb("prepare the storage health query", error)
      })?;
      let rows = statement
        .query_map([], |row| {
          Ok(StoredRow {
            device_id: row.get(0)?,
            display_name: row.get(1)?,
            model: row.get(2)?,
            protocol: row.get(3)?,
            capacity_bytes: row.get(4)?,
            date: row.get(5)?,
            health_status: row.get(6)?,
            warning_level: row.get(7)?,
            warning_reasons: row.get(8)?,
            temperature_celsius: row.get(9)?,
            power_on_hours: row.get(10)?,
            percentage_used: row.get(11)?,
            available_spare_percent: row.get(12)?,
            reallocated_sector_count: row.get(13)?,
            current_pending_sector_count: row.get(14)?,
            offline_uncorrectable_count: row.get(15)?,
            media_errors: row.get(16)?,
            error_log_entries: row.get(17)?,
            unsafe_shutdown_count: row.get(18)?,
            collected_at: row.get(19)?,
          })
        })
        .map_err(|error| {
          NativeDatabaseError::duckdb("run the storage health query", error)
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
          NativeDatabaseError::duckdb("decode a storage health row", error)
        })?;
      Ok(rows.into_iter().map(StoredRow::into_record).collect())
    })
    .await
}

/// One row of [`latest_records`] as the native columns carry it, before the
/// SQLite reader's own narrowing is applied.
struct StoredRow {
  device_id: String,
  display_name: String,
  model: Option<String>,
  protocol: Option<String>,
  capacity_bytes: Option<i64>,
  date: String,
  health_status: String,
  warning_level: String,
  warning_reasons: Option<String>,
  temperature_celsius: Option<f64>,
  power_on_hours: Option<i64>,
  percentage_used: Option<f64>,
  available_spare_percent: Option<f64>,
  reallocated_sector_count: Option<i64>,
  current_pending_sector_count: Option<i64>,
  offline_uncorrectable_count: Option<i64>,
  media_errors: Option<i64>,
  error_log_entries: Option<i64>,
  unsafe_shutdown_count: Option<i64>,
  collected_at: String,
}

impl StoredRow {
  /// The narrowing the sqlx reader performs, restated over the same values:
  /// a negative or oversized count becomes `None` rather than a wrapped
  /// number, a REAL is narrowed back to the `f32` the collector produced, and
  /// `warning_reasons` that is not a JSON array of strings degrades to an
  /// empty list rather than failing the whole read.
  fn into_record(self) -> StorageHealthRecord {
    StorageHealthRecord {
      device_id: self.device_id,
      display_name: self.display_name,
      model: self.model,
      protocol: self.protocol,
      capacity_bytes: from_i64(self.capacity_bytes),
      date: self.date,
      health_status: parse_health_status(&self.health_status),
      warning_level: parse_warning_level(&self.warning_level),
      temperature_celsius: self.temperature_celsius.map(|value| value as f32),
      power_on_hours: from_i64(self.power_on_hours),
      percentage_used: self.percentage_used.map(|value| value as f32),
      available_spare_percent: self.available_spare_percent.map(|value| value as f32),
      reallocated_sector_count: from_i64(self.reallocated_sector_count),
      current_pending_sector_count: from_i64(self.current_pending_sector_count),
      offline_uncorrectable_count: from_i64(self.offline_uncorrectable_count),
      media_errors: from_i64(self.media_errors),
      error_log_entries: from_i64(self.error_log_entries),
      unsafe_shutdown_count: from_i64(self.unsafe_shutdown_count),
      warning_reasons: self
        .warning_reasons
        .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
        .unwrap_or_default(),
      collected_at: self.collected_at,
    }
  }
}

/// The devices, then the records, inside a caller's transaction.
///
/// The `devices.is_empty() || records.is_empty()` guard is the SQLite one:
/// device rows are only written as the identity half of a record, so a
/// collection that produced no records writes no devices either.
fn insert_in(
  transaction: &NativeTransactionContext<'_, '_>,
  devices: &[StorageDeviceRecord],
  records: &[StorageHealthRecordDraft],
) -> Result<(), NativeDatabaseError> {
  if devices.is_empty() || records.is_empty() {
    return Ok(());
  }
  let connection = transaction.connection();

  // `serial_hash` is the one column the conflict clause does not simply
  // overwrite: a collector running without the privileges that expose the
  // serial number reports `None`, and forgetting the hash the privileged run
  // established would break the device identity a returning disk is
  // recognised by. `first_seen_at` is likewise absent from the update set -
  // it is the first sighting, not the latest one.
  let mut device_statement = connection
    .prepare(
      "INSERT INTO storage_devices (
         id,
         display_name,
         model,
         serial_hash,
         protocol,
         capacity_bytes,
         first_seen_at,
         last_seen_at,
         is_active
       )
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1)
       ON CONFLICT (id) DO UPDATE SET
         display_name = excluded.display_name,
         model = excluded.model,
         serial_hash = COALESCE(excluded.serial_hash, storage_devices.serial_hash),
         protocol = excluded.protocol,
         capacity_bytes = excluded.capacity_bytes,
         last_seen_at = excluded.last_seen_at,
         is_active = 1",
    )
    .map_err(|error| {
      NativeDatabaseError::duckdb("prepare the storage device upsert", error)
    })?;
  for device in devices {
    transaction.check_cancelled()?;
    device_statement
      .execute(params![
        device.id.as_str(),
        device.display_name.as_str(),
        device.model.as_deref(),
        device.serial_hash.as_deref(),
        device.protocol.as_deref(),
        to_i64(device.capacity_bytes),
        device.first_seen_at.as_str(),
        device.last_seen_at.as_str()
      ])
      .map_err(|error| NativeDatabaseError::duckdb("upsert a storage device", error))?;
  }

  let mut record_statement = connection
    .prepare(
      "INSERT INTO storage_health_daily_records (
         id,
         device_id,
         date,
         health_status,
         warning_level,
         warning_reasons,
         temperature_celsius,
         power_on_hours,
         percentage_used,
         available_spare_percent,
         reallocated_sector_count,
         current_pending_sector_count,
         offline_uncorrectable_count,
         media_errors,
         error_log_entries,
         unsafe_shutdown_count,
         collected_at
       )
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
       ON CONFLICT (device_id, date) DO UPDATE SET
         health_status = excluded.health_status,
         warning_level = excluded.warning_level,
         warning_reasons = excluded.warning_reasons,
         temperature_celsius = excluded.temperature_celsius,
         power_on_hours = excluded.power_on_hours,
         percentage_used = excluded.percentage_used,
         available_spare_percent = excluded.available_spare_percent,
         reallocated_sector_count = excluded.reallocated_sector_count,
         current_pending_sector_count = excluded.current_pending_sector_count,
         offline_uncorrectable_count = excluded.offline_uncorrectable_count,
         media_errors = excluded.media_errors,
         error_log_entries = excluded.error_log_entries,
         unsafe_shutdown_count = excluded.unsafe_shutdown_count,
         collected_at = excluded.collected_at",
    )
    .map_err(|error| {
      NativeDatabaseError::duckdb("prepare the storage health record upsert", error)
    })?;
  for record in records {
    transaction.check_cancelled()?;
    let id = transaction.next_id(RECORDS_TABLE)?;
    let warning_reasons =
      serde_json::to_string(&record.warning_reasons).unwrap_or_else(|_| "[]".into());
    // Every real column here is nullable, so a NaN reading becomes the gap
    // SQLite would have stored rather than a NaN no SQLite row can hold.
    let reading =
      |value: Option<f32>| value.and_then(|value| sqlite_real_binding(f64::from(value)));
    record_statement
      .execute(params![
        id,
        record.device_id.as_str(),
        record.date.as_str(),
        record.health_status.as_str(),
        record.warning_level.as_str(),
        warning_reasons.as_str(),
        reading(record.temperature_celsius),
        to_i64(record.power_on_hours),
        reading(record.percentage_used),
        reading(record.available_spare_percent),
        to_i64(record.reallocated_sector_count),
        to_i64(record.current_pending_sector_count),
        to_i64(record.offline_uncorrectable_count),
        to_i64(record.media_errors),
        to_i64(record.error_log_entries),
        to_i64(record.unsafe_shutdown_count),
        record.collected_at.as_str()
      ])
      .map_err(|error| {
        NativeDatabaseError::duckdb("upsert a storage health record", error)
      })?;
  }
  Ok(())
}

/// Clear every active flag and re-set the ones the caller enumerated.
///
/// Empty means "nothing was enumerated", which is uncertainty rather than
/// evidence that every disk went away, so it clears nothing - the SQLite
/// guard, kept.
fn update_active_devices_in(
  transaction: &NativeTransactionContext<'_, '_>,
  active_device_ids: &[String],
) -> Result<(), NativeDatabaseError> {
  if active_device_ids.is_empty() {
    return Ok(());
  }
  let connection = transaction.connection();
  connection
    .execute("UPDATE storage_devices SET is_active = 0", [])
    .map_err(|error| {
      NativeDatabaseError::duckdb("clear the storage device active flags", error)
    })?;
  let mut statement = connection
    .prepare("UPDATE storage_devices SET is_active = 1 WHERE id = ?")
    .map_err(|error| {
      NativeDatabaseError::duckdb("prepare the storage device activation", error)
    })?;
  for device_id in active_device_ids {
    transaction.check_cancelled()?;
    statement
      .execute(params![device_id.as_str()])
      .map_err(|error| NativeDatabaseError::duckdb("activate a storage device", error))?;
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  /// The shape only; that it *agrees with SQLite's `NOCASE`* is measured
  /// against sqlx in `core/tests/duckdb_storage_health.rs`, which is the
  /// claim that matters.
  #[test]
  fn folds_only_the_ascii_letters() {
    assert_eq!(
      sqlite_nocase_key("name"),
      "translate(name, 'ABCDEFGHIJKLMNOPQRSTUVWXYZ', 'abcdefghijklmnopqrstuvwxyz')"
    );
  }
}
