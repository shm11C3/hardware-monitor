use super::db;
use crate::models::hardware::{
  StorageDeviceRecord, StorageHealthRecord, StorageHealthRecordDraft,
  StorageHealthStatus, StorageWarningLevel,
};
use sqlx::Row;

pub async fn insert_daily_records(
  devices: Vec<StorageDeviceRecord>,
  records: Vec<StorageHealthRecordDraft>,
) -> Result<(), sqlx::Error> {
  let pool = db::get_pool().await?;
  let mut tx = pool.begin().await?;

  insert_daily_records_in_transaction(&mut tx, devices, records).await?;

  tx.commit().await?;
  Ok(())
}

pub async fn refresh_daily_records(
  active_device_ids: &[String],
  devices: Vec<StorageDeviceRecord>,
  records: Vec<StorageHealthRecordDraft>,
) -> Result<(), sqlx::Error> {
  if active_device_ids.is_empty() && (devices.is_empty() || records.is_empty()) {
    return Ok(());
  }

  let pool = db::get_pool().await?;
  let mut tx = pool.begin().await?;

  insert_daily_records_in_transaction(&mut tx, devices, records).await?;
  update_active_devices_in_transaction(&mut tx, active_device_ids).await?;

  tx.commit().await?;
  Ok(())
}

pub async fn delete_old_data(retention_days: u32) -> Result<(), sqlx::Error> {
  let pool = db::get_pool().await?;
  let cutoff = (chrono::Local::now().date_naive()
    - chrono::Duration::days(retention_days as i64))
  .format("%Y-%m-%d")
  .to_string();

  sqlx::query("DELETE FROM storage_health_daily_records WHERE date < $1")
    .bind(cutoff)
    .execute(&pool)
    .await?;

  Ok(())
}

pub async fn latest_records() -> Result<Vec<StorageHealthRecord>, sqlx::Error> {
  let pool = db::get_pool().await?;
  latest_records_from_pool(&pool).await
}

/// Reads the most recent health record per active device.
///
/// The `storage_health_daily_records` table is created by an app-side
/// migration. When a database predates that migration — e.g. an older build
/// without the storage-health schema was the last to open this file — the
/// table does not exist yet. In that case we return an empty result instead of
/// surfacing a hard `no such table` error: there genuinely are no records, and
/// the rows appear once the migration runs. This mirrors the startup preflight
/// check, which also treats a missing migration table as compatible.
async fn latest_records_from_pool(
  pool: &sqlx::SqlitePool,
) -> Result<Vec<StorageHealthRecord>, sqlx::Error> {
  let rows = match sqlx::query(
    r#"
    SELECT
      s.device_id,
      COALESCE(d.display_name, s.device_id) AS display_name,
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
      display_name COLLATE NOCASE
    "#,
  )
  .fetch_all(pool)
  .await
  {
    Ok(rows) => rows,
    Err(error) if is_missing_table_error(&error) => return Ok(Vec::new()),
    Err(error) => return Err(error),
  };

  rows
    .into_iter()
    .map(|row| {
      let warning_reasons = row
        .try_get::<Option<String>, _>("warning_reasons")?
        .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
        .unwrap_or_default();

      Ok(StorageHealthRecord {
        device_id: row.try_get("device_id")?,
        display_name: row.try_get("display_name")?,
        model: row.try_get("model")?,
        protocol: row.try_get("protocol")?,
        capacity_bytes: from_i64(row.try_get("capacity_bytes")?),
        date: row.try_get("date")?,
        health_status: parse_health_status(&row.try_get::<String, _>("health_status")?),
        warning_level: parse_warning_level(&row.try_get::<String, _>("warning_level")?),
        temperature_celsius: row
          .try_get::<Option<f64>, _>("temperature_celsius")?
          .map(|value| value as f32),
        power_on_hours: from_i64(row.try_get("power_on_hours")?),
        percentage_used: row
          .try_get::<Option<f64>, _>("percentage_used")?
          .map(|value| value as f32),
        available_spare_percent: row
          .try_get::<Option<f64>, _>("available_spare_percent")?
          .map(|value| value as f32),
        reallocated_sector_count: from_i64(row.try_get("reallocated_sector_count")?),
        current_pending_sector_count: from_i64(
          row.try_get("current_pending_sector_count")?,
        ),
        offline_uncorrectable_count: from_i64(
          row.try_get("offline_uncorrectable_count")?,
        ),
        media_errors: from_i64(row.try_get("media_errors")?),
        error_log_entries: from_i64(row.try_get("error_log_entries")?),
        unsafe_shutdown_count: from_i64(row.try_get("unsafe_shutdown_count")?),
        warning_reasons,
        collected_at: row.try_get("collected_at")?,
      })
    })
    .collect()
}

async fn insert_daily_records_in_transaction(
  tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
  devices: Vec<StorageDeviceRecord>,
  records: Vec<StorageHealthRecordDraft>,
) -> Result<(), sqlx::Error> {
  if devices.is_empty() || records.is_empty() {
    return Ok(());
  }

  for device in devices {
    sqlx::query(
      r#"
      INSERT INTO storage_devices (
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
      VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 1)
      ON CONFLICT(id) DO UPDATE SET
        display_name = excluded.display_name,
        model = excluded.model,
        serial_hash = COALESCE(excluded.serial_hash, storage_devices.serial_hash),
        protocol = excluded.protocol,
        capacity_bytes = excluded.capacity_bytes,
        last_seen_at = excluded.last_seen_at,
        is_active = 1
      "#,
    )
    .bind(device.id)
    .bind(device.display_name)
    .bind(device.model)
    .bind(device.serial_hash)
    .bind(device.protocol)
    .bind(to_i64(device.capacity_bytes))
    .bind(device.first_seen_at)
    .bind(device.last_seen_at)
    .execute(&mut **tx)
    .await?;
  }

  for record in records {
    let warning_reasons =
      serde_json::to_string(&record.warning_reasons).unwrap_or_else(|_| "[]".into());

    sqlx::query(
      r#"
      INSERT INTO storage_health_daily_records (
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
      VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
      ON CONFLICT(device_id, date) DO UPDATE SET
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
        collected_at = excluded.collected_at
      "#,
    )
    .bind(record.device_id)
    .bind(record.date)
    .bind(record.health_status.as_str())
    .bind(record.warning_level.as_str())
    .bind(warning_reasons)
    .bind(record.temperature_celsius.map(f64::from))
    .bind(to_i64(record.power_on_hours))
    .bind(record.percentage_used.map(f64::from))
    .bind(record.available_spare_percent.map(f64::from))
    .bind(to_i64(record.reallocated_sector_count))
    .bind(to_i64(record.current_pending_sector_count))
    .bind(to_i64(record.offline_uncorrectable_count))
    .bind(to_i64(record.media_errors))
    .bind(to_i64(record.error_log_entries))
    .bind(to_i64(record.unsafe_shutdown_count))
    .bind(record.collected_at)
    .execute(&mut **tx)
    .await?;
  }

  Ok(())
}

async fn update_active_devices_in_transaction(
  tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
  active_device_ids: &[String],
) -> Result<(), sqlx::Error> {
  if active_device_ids.is_empty() {
    return Ok(());
  }

  sqlx::query("UPDATE storage_devices SET is_active = 0")
    .execute(&mut **tx)
    .await?;

  for device_id in active_device_ids {
    sqlx::query("UPDATE storage_devices SET is_active = 1 WHERE id = $1")
      .bind(device_id)
      .execute(&mut **tx)
      .await?;
  }

  Ok(())
}

/// Returns `true` when the error is SQLite's "no such table" error, signalling
/// that the storage-health schema migration has not run against this database
/// yet. Matched on the message string to stay consistent with the startup
/// preflight check in `crate::persistence::preflight`.
fn is_missing_table_error(error: &sqlx::Error) -> bool {
  error.to_string().contains("no such table")
}

/// `pub(crate)` so the native Storage Health family reuses this rule rather
/// than restating it: an id too large for a signed 64-bit integer is stored
/// as NULL, which is what SQLite would have received.
pub(crate) fn to_i64(value: Option<u64>) -> Option<i64> {
  value.and_then(|v| i64::try_from(v).ok())
}

pub(crate) fn from_i64(value: Option<i64>) -> Option<u64> {
  value.and_then(|v| u64::try_from(v).ok())
}

pub(crate) fn parse_health_status(value: &str) -> StorageHealthStatus {
  match value {
    "good" => StorageHealthStatus::Good,
    "warning" => StorageHealthStatus::Warning,
    "critical" => StorageHealthStatus::Critical,
    _ => StorageHealthStatus::Unknown,
  }
}

pub(crate) fn parse_warning_level(value: &str) -> StorageWarningLevel {
  match value {
    "none" => StorageWarningLevel::None,
    "warning" => StorageWarningLevel::Warning,
    "critical" => StorageWarningLevel::Critical,
    _ => StorageWarningLevel::Unknown,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::infrastructure::database::db;

  async fn setup_test_db() -> tempfile::NamedTempFile {
    let file = tempfile::NamedTempFile::new().unwrap();
    assert!(db::init(file.path().to_path_buf()));

    let pool = db::get_pool().await.unwrap();
    sqlx::query(
      r#"
      CREATE TABLE storage_devices (
        id TEXT PRIMARY KEY,
        display_name TEXT NOT NULL,
        model TEXT,
        serial_hash TEXT,
        protocol TEXT,
        capacity_bytes INTEGER,
        first_seen_at TEXT NOT NULL,
        last_seen_at TEXT NOT NULL,
        is_active INTEGER NOT NULL DEFAULT 1
      )
      "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
      r#"
      CREATE TABLE storage_health_daily_records (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        device_id TEXT NOT NULL,
        date TEXT NOT NULL,
        health_status TEXT NOT NULL,
        warning_level TEXT NOT NULL DEFAULT 'none',
        warning_reasons TEXT,
        temperature_celsius REAL,
        power_on_hours INTEGER,
        percentage_used REAL,
        available_spare_percent REAL,
        reallocated_sector_count INTEGER,
        current_pending_sector_count INTEGER,
        offline_uncorrectable_count INTEGER,
        media_errors INTEGER,
        error_log_entries INTEGER,
        unsafe_shutdown_count INTEGER,
        collected_at TEXT NOT NULL,
        UNIQUE(device_id, date),
        FOREIGN KEY(device_id) REFERENCES storage_devices(id)
      )
      "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    file
  }

  fn device_record(id: &str) -> StorageDeviceRecord {
    StorageDeviceRecord {
      id: id.to_string(),
      display_name: format!("Device {id}"),
      model: Some("Example SSD".to_string()),
      serial_hash: Some(format!("serial-{id}")),
      protocol: Some("NVMe".to_string()),
      capacity_bytes: Some(1_000_000),
      first_seen_at: "2026-05-10T00:00:00Z".to_string(),
      last_seen_at: "2026-05-10T00:00:00Z".to_string(),
    }
  }

  fn record(device_id: &str, date: &str) -> StorageHealthRecordDraft {
    StorageHealthRecordDraft {
      device_id: device_id.to_string(),
      date: date.to_string(),
      health_status: StorageHealthStatus::Good,
      warning_level: StorageWarningLevel::None,
      temperature_celsius: Some(38.0),
      power_on_hours: Some(120),
      percentage_used: None,
      available_spare_percent: None,
      reallocated_sector_count: None,
      current_pending_sector_count: None,
      offline_uncorrectable_count: None,
      media_errors: None,
      error_log_entries: None,
      unsafe_shutdown_count: None,
      warning_reasons: vec!["test reason".to_string()],
      collected_at: "2026-05-10T00:00:00Z".to_string(),
    }
  }

  #[tokio::test]
  async fn latest_records_returns_empty_when_table_missing() {
    // A database that predates the storage-health migration has no
    // `storage_health_daily_records` table. Reading must degrade to an empty
    // result rather than surfacing a hard `no such table` error. Uses a fresh
    // in-memory pool to avoid touching the process-wide `db::init` path.
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
      .await
      .expect("in-memory sqlite pool");

    let records = latest_records_from_pool(&pool)
      .await
      .expect("missing table must not be an error");

    assert!(records.is_empty());
  }

  #[tokio::test]
  async fn insert_daily_records_refreshes_same_day_and_preserves_active_flags() {
    let _dir = setup_test_db().await;
    let pool = db::get_pool().await.unwrap();

    sqlx::query(
      r#"
      INSERT INTO storage_devices (
        id, display_name, first_seen_at, last_seen_at, is_active
      )
      VALUES
        ('disk-b', 'Device disk-b', '2026-05-09T00:00:00Z', '2026-05-09T00:00:00Z', 0),
        ('disk-c', 'Device disk-c', '2026-05-09T00:00:00Z', '2026-05-09T00:00:00Z', 1)
      "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    // First collection of the day failed to read health data (e.g. all
    // collectors fell back to minimal CIM data without elevation).
    insert_daily_records(
      vec![device_record("disk-a")],
      vec![StorageHealthRecordDraft {
        health_status: StorageHealthStatus::Unknown,
        warning_level: StorageWarningLevel::Unknown,
        temperature_celsius: None,
        warning_reasons: Vec::new(),
        ..record("disk-a", "2026-05-10")
      }],
    )
    .await
    .unwrap();
    // A later collection on the same day succeeded with full health data.
    insert_daily_records(
      vec![device_record("disk-a"), device_record("disk-b")],
      vec![
        record("disk-a", "2026-05-10"),
        record("disk-b", "2026-05-10"),
      ],
    )
    .await
    .unwrap();

    let total_for_date: (i64,) =
      sqlx::query_as("SELECT COUNT(1) FROM storage_health_daily_records WHERE date = $1")
        .bind("2026-05-10")
        .fetch_one(&pool)
        .await
        .unwrap();
    let disk_a_count: (i64,) = sqlx::query_as(
      "SELECT COUNT(1) FROM storage_health_daily_records WHERE device_id = $1 AND date = $2",
    )
    .bind("disk-a")
    .bind("2026-05-10")
    .fetch_one(&pool)
    .await
    .unwrap();
    let disk_b_active: (i64,) =
      sqlx::query_as("SELECT is_active FROM storage_devices WHERE id = $1")
        .bind("disk-b")
        .fetch_one(&pool)
        .await
        .unwrap();
    let disk_c_active: (i64,) =
      sqlx::query_as("SELECT is_active FROM storage_devices WHERE id = $1")
        .bind("disk-c")
        .fetch_one(&pool)
        .await
        .unwrap();
    let warning_reasons: (String,) = sqlx::query_as(
      "SELECT warning_reasons FROM storage_health_daily_records WHERE device_id = $1 AND date = $2",
    )
    .bind("disk-b")
    .bind("2026-05-10")
    .fetch_one(&pool)
    .await
    .unwrap();

    let disk_a_row: (String, Option<f64>) = sqlx::query_as(
      "SELECT health_status, temperature_celsius FROM storage_health_daily_records WHERE device_id = $1 AND date = $2",
    )
    .bind("disk-a")
    .bind("2026-05-10")
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(total_for_date.0, 2);
    assert_eq!(disk_a_count.0, 1);
    // The same-day record is refreshed with the latest collection result.
    assert_eq!(disk_a_row.0, "good");
    assert_eq!(disk_a_row.1, Some(38.0));
    assert_eq!(disk_b_active.0, 1);
    assert_eq!(disk_c_active.0, 1);
    assert_eq!(warning_reasons.0, r#"["test reason"]"#);

    insert_daily_records(
      vec![device_record("disk-c")],
      vec![StorageHealthRecordDraft {
        health_status: StorageHealthStatus::Critical,
        warning_level: StorageWarningLevel::Critical,
        current_pending_sector_count: Some(2),
        warning_reasons: vec![
          "Current pending sectors are present".to_string(),
          "Offline uncorrectable sectors are present".to_string(),
        ],
        ..record("disk-c", "2026-05-11")
      }],
    )
    .await
    .unwrap();

    let latest = latest_records().await.unwrap();
    assert_eq!(latest.len(), 3);
    assert_eq!(latest[0].device_id, "disk-c");
    assert_eq!(latest[0].display_name, "Device disk-c");
    assert_eq!(latest[0].date, "2026-05-11");
    assert_eq!(latest[0].health_status, StorageHealthStatus::Critical);
    assert_eq!(latest[0].warning_level, StorageWarningLevel::Critical);
    assert_eq!(latest[0].current_pending_sector_count, Some(2));
    assert_eq!(
      latest[0].warning_reasons,
      vec![
        "Current pending sectors are present".to_string(),
        "Offline uncorrectable sectors are present".to_string(),
      ]
    );
    assert!(
      latest
        .iter()
        .any(|record| record.device_id == "disk-a" && record.date == "2026-05-10")
    );
    assert!(
      latest
        .iter()
        .any(|record| record.device_id == "disk-b" && record.date == "2026-05-10")
    );

    refresh_daily_records(
      &["disk-a".to_string(), "disk-c".to_string()],
      Vec::new(),
      Vec::new(),
    )
    .await
    .unwrap();

    let disk_b_active_after_removal: (i64,) =
      sqlx::query_as("SELECT is_active FROM storage_devices WHERE id = $1")
        .bind("disk-b")
        .fetch_one(&pool)
        .await
        .unwrap();
    let latest_after_removal = latest_records().await.unwrap();

    assert_eq!(disk_b_active_after_removal.0, 0);
    assert_eq!(latest_after_removal.len(), 2);
    assert!(
      !latest_after_removal
        .iter()
        .any(|record| record.device_id == "disk-b")
    );

    refresh_daily_records(&[], Vec::new(), Vec::new())
      .await
      .unwrap();

    let inactive_count: (i64,) =
      sqlx::query_as("SELECT COUNT(1) FROM storage_devices WHERE is_active = 0")
        .fetch_one(&pool)
        .await
        .unwrap();
    let latest_after_empty_refresh = latest_records().await.unwrap();

    assert_eq!(inactive_count.0, 1);
    assert_eq!(latest_after_empty_refresh.len(), 2);

    refresh_daily_records(
      &["disk-a".to_string()],
      vec![device_record("disk-b")],
      vec![record("disk-b", "2026-05-12")],
    )
    .await
    .unwrap();

    let disk_b_active_after_authoritative_refresh: (i64,) =
      sqlx::query_as("SELECT is_active FROM storage_devices WHERE id = $1")
        .bind("disk-b")
        .fetch_one(&pool)
        .await
        .unwrap();
    let latest_after_authoritative_refresh = latest_records().await.unwrap();

    assert_eq!(disk_b_active_after_authoritative_refresh.0, 0);
    assert!(
      !latest_after_authoritative_refresh
        .iter()
        .any(|record| record.device_id == "disk-b")
    );

    refresh_daily_records(
      &["disk-a".to_string(), "disk-b".to_string()],
      vec![device_record("disk-b")],
      vec![record("disk-b", "2026-05-13")],
    )
    .await
    .unwrap();

    let disk_b_active: (i64,) =
      sqlx::query_as("SELECT is_active FROM storage_devices WHERE id = $1")
        .bind("disk-b")
        .fetch_one(&pool)
        .await
        .unwrap();
    let disk_b_history_count: (i64,) = sqlx::query_as(
      "SELECT COUNT(1) FROM storage_health_daily_records WHERE device_id = $1",
    )
    .bind("disk-b")
    .fetch_one(&pool)
    .await
    .unwrap();
    let latest = latest_records().await.unwrap();

    assert_eq!(disk_b_active.0, 1);
    assert_eq!(disk_b_history_count.0, 3);
    assert!(
      latest
        .iter()
        .any(|record| record.device_id == "disk-b" && record.date == "2026-05-13")
    );
  }
}
