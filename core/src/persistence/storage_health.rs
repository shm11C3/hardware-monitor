//! Daily storage health record worker.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{Duration, MissedTickBehavior, interval};

use crate::infrastructure::database;
use crate::models::hardware::{
  SmartAttribute, SmartDiskInfo, SmartHealthStatus, StorageDeviceRecord,
  StorageHealthRecordDraft, StorageHealthStatus, StorageWarningLevel,
};
use crate::models::{ExternalComponentGuidanceCandidate, SmartInfoCollectionOutcome};
use crate::settings::STORAGE_HEALTH_IDENTITY_HASH_KEY_BYTES;
use crate::{log_error, log_info};

const STORAGE_HEALTH_CHECK_INTERVAL_SECONDS: u64 = 60;
const TEMPERATURE_WARNING_CELSIUS: f32 = 55.0;
const TEMPERATURE_CRITICAL_CELSIUS: f32 = 70.0;
const PERCENTAGE_USED_WARNING: f32 = 80.0;
const PERCENTAGE_USED_CRITICAL: f32 = 100.0;
const AVAILABLE_SPARE_WARNING_PERCENT: f32 = 20.0;
const AVAILABLE_SPARE_CRITICAL_PERCENT: f32 = 10.0;

pub type ExternalComponentGuidanceSink =
  Arc<dyn Fn(Vec<ExternalComponentGuidanceCandidate>) + Send + Sync + 'static>;

struct StorageHealthCollection {
  devices: Vec<StorageDeviceRecord>,
  records: Vec<StorageHealthRecordDraft>,
}

pub struct StorageHealthController {
  handle: JoinHandle<()>,
  stop_tx: watch::Sender<bool>,
}

impl StorageHealthController {
  pub fn setup(
    runtime: Handle,
    retention_days: u32,
    identity_hash_key: [u8; STORAGE_HEALTH_IDENTITY_HASH_KEY_BYTES],
  ) -> Self {
    Self::setup_with_guidance_sink(runtime, retention_days, identity_hash_key, None)
  }

  pub fn setup_with_guidance_sink(
    runtime: Handle,
    retention_days: u32,
    identity_hash_key: [u8; STORAGE_HEALTH_IDENTITY_HASH_KEY_BYTES],
    guidance_sink: Option<ExternalComponentGuidanceSink>,
  ) -> Self {
    let (stop_tx, mut stop_rx) = watch::channel(false);

    let handle = runtime.spawn(async move {
      let today = local_date_string();
      run_daily_record_for_date(
        retention_days,
        &today,
        &identity_hash_key,
        guidance_sink.as_ref(),
      )
      .await;
      let mut last_checked_date = Some(today);

      let mut ticker =
        interval(Duration::from_secs(STORAGE_HEALTH_CHECK_INTERVAL_SECONDS));
      ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
      ticker.tick().await;

      loop {
        tokio::select! {
          biased;
          changed = stop_rx.changed() => {
            if changed.is_err() || *stop_rx.borrow() {
              log_info!(
                "storage health worker shutdown signal received",
                "persistence::storage_health",
                None::<&str>
              );
              break;
            }
          }
          _ = ticker.tick() => {
            let today = local_date_string();
            if last_checked_date.as_deref() != Some(today.as_str()) {
              run_daily_record_for_date(
                retention_days,
                &today,
                &identity_hash_key,
                guidance_sink.as_ref(),
              ).await;
              last_checked_date = Some(today);
            }
          }
        }
      }
    });

    Self { handle, stop_tx }
  }

  pub async fn terminate(self) {
    let _ = self.stop_tx.send(true);
    let _ = self.handle.await;
  }
}

async fn run_daily_record_for_date(
  retention_days: u32,
  date: &str,
  identity_hash_key: &[u8; STORAGE_HEALTH_IDENTITY_HASH_KEY_BYTES],
  guidance_sink: Option<&ExternalComponentGuidanceSink>,
) {
  let collection =
    match collect_storage_health_for_date(date, identity_hash_key, guidance_sink).await {
      Ok(collection) => collection,
      Err(e) => {
        log_error!(
          "Failed to collect storage health info",
          "persistence::storage_health::run_daily_record_for_date",
          Some(e)
        );
        return;
      }
    };

  if let Err(e) =
    store_storage_health_collection(retention_days, Vec::new(), collection).await
  {
    log_error!(
      "Failed to store storage health daily records",
      "persistence::storage_health::run_daily_record_for_date",
      Some(e)
    );
  }
}

pub async fn refresh_storage_health_for_date(
  retention_days: u32,
  date: &str,
  identity_hash_key: &[u8; STORAGE_HEALTH_IDENTITY_HASH_KEY_BYTES],
  active_device_ids: Vec<String>,
  guidance_sink: Option<&ExternalComponentGuidanceSink>,
) -> Result<(), String> {
  let collection =
    collect_storage_health_for_date(date, identity_hash_key, guidance_sink).await?;
  store_storage_health_collection(retention_days, active_device_ids, collection).await
}

pub fn local_storage_health_date_string() -> String {
  local_date_string()
}

async fn collect_storage_health_for_date(
  date: &str,
  identity_hash_key: &[u8; STORAGE_HEALTH_IDENTITY_HASH_KEY_BYTES],
  guidance_sink: Option<&ExternalComponentGuidanceSink>,
) -> Result<StorageHealthCollection, String> {
  let collected_at = chrono::Utc::now().to_rfc3339();
  let outcome = tokio::task::spawn_blocking(collect_platform_smart_info_with_guidance)
    .await
    .map_err(|e| format!("Failed to join storage health collection task: {e}"))?;

  if !outcome.guidance_candidates.is_empty()
    && let Some(sink) = guidance_sink
  {
    sink(outcome.guidance_candidates.clone());
  }

  let disks = outcome.disks?;

  let (devices, records): (Vec<_>, Vec<_>) = disks
    .iter()
    .map(|disk| build_daily_record(disk, date, &collected_at, identity_hash_key))
    .unzip();

  if records.is_empty() {
    return Err("Storage Health collection returned no disks".to_string());
  }

  Ok(StorageHealthCollection { devices, records })
}

async fn store_storage_health_collection(
  retention_days: u32,
  active_device_ids: Vec<String>,
  collection: StorageHealthCollection,
) -> Result<(), String> {
  let active_device_ids =
    active_device_ids_with_collected_devices(active_device_ids, &collection.devices);
  let result = if active_device_ids.is_empty() {
    database::storage_health::insert_daily_records(collection.devices, collection.records)
      .await
  } else {
    database::storage_health::refresh_daily_records(
      &active_device_ids,
      collection.devices,
      collection.records,
    )
    .await
  };

  result.map_err(|e| format!("Failed to insert storage health daily records: {e}"))?;

  database::storage_health::delete_old_data(retention_days)
    .await
    .map_err(|e| format!("Failed to delete old storage health records: {e}"))
}

fn active_device_ids_with_collected_devices(
  mut active_device_ids: Vec<String>,
  devices: &[StorageDeviceRecord],
) -> Vec<String> {
  if active_device_ids.is_empty() {
    return active_device_ids;
  }

  for device in devices {
    if !active_device_ids.iter().any(|id| id == &device.id) {
      active_device_ids.push(device.id.clone());
    }
  }

  active_device_ids
}

fn build_daily_record(
  disk: &SmartDiskInfo,
  date: &str,
  collected_at: &str,
  identity_hash_key: &[u8; STORAGE_HEALTH_IDENTITY_HASH_KEY_BYTES],
) -> (StorageDeviceRecord, StorageHealthRecordDraft) {
  let device_id = storage_device_id(disk, identity_hash_key);
  let display_name = disk
    .model_name
    .as_deref()
    .filter(|v| !v.trim().is_empty())
    .unwrap_or(&disk.device_name)
    .to_string();
  let serial_hash =
    normalized_serial(disk).map(|serial| hash_identifier(serial, identity_hash_key));

  let temperature_celsius = disk.temperature_celsius.map(|v| v as f32);
  let percentage_used = attr_value_f32(disk, None, "Percentage Used");
  let available_spare_percent = attr_value_f32(disk, None, "Available Spare");
  let reallocated_sector_count =
    attr_value_u64(disk, Some(5), "Reallocated Sector Count");
  let current_pending_sector_count =
    attr_value_u64(disk, Some(197), "Current Pending Sector Count");
  let offline_uncorrectable_count =
    attr_value_u64(disk, Some(198), "Offline Uncorrectable Sector Count");
  let media_errors = attr_value_u64(disk, None, "Media Errors");
  let error_log_entries = attr_value_u64(disk, None, "Error Information Log Entries");
  let unsafe_shutdown_count = attr_value_u64(disk, None, "Unsafe Shutdowns");

  let (health_status, warning_level, warning_reasons) =
    classify_record_warnings(RecordSignals {
      smart_health_status: &disk.health_status,
      temperature_celsius,
      percentage_used,
      available_spare_percent,
      reallocated_sector_count,
      current_pending_sector_count,
      offline_uncorrectable_count,
      media_errors,
    });

  (
    StorageDeviceRecord {
      id: device_id.clone(),
      display_name,
      model: disk.model_name.clone(),
      serial_hash,
      protocol: disk.protocol.clone().or_else(|| disk.device_type.clone()),
      capacity_bytes: disk.capacity_bytes,
      first_seen_at: collected_at.to_string(),
      last_seen_at: collected_at.to_string(),
    },
    StorageHealthRecordDraft {
      device_id,
      date: date.to_string(),
      health_status,
      warning_level,
      temperature_celsius,
      power_on_hours: disk.power_on_hours,
      percentage_used,
      available_spare_percent,
      reallocated_sector_count,
      current_pending_sector_count,
      offline_uncorrectable_count,
      media_errors,
      error_log_entries,
      unsafe_shutdown_count,
      warning_reasons,
      collected_at: collected_at.to_string(),
    },
  )
}

struct RecordSignals<'a> {
  smart_health_status: &'a SmartHealthStatus,
  temperature_celsius: Option<f32>,
  percentage_used: Option<f32>,
  available_spare_percent: Option<f32>,
  reallocated_sector_count: Option<u64>,
  current_pending_sector_count: Option<u64>,
  offline_uncorrectable_count: Option<u64>,
  media_errors: Option<u64>,
}

fn classify_record_warnings(
  signals: RecordSignals<'_>,
) -> (StorageHealthStatus, StorageWarningLevel, Vec<String>) {
  let mut level = match signals.smart_health_status {
    SmartHealthStatus::Passed => StorageWarningLevel::None,
    SmartHealthStatus::Failed => StorageWarningLevel::Critical,
    SmartHealthStatus::Unknown => StorageWarningLevel::Unknown,
  };
  let mut reasons = Vec::new();

  if matches!(signals.smart_health_status, SmartHealthStatus::Failed) {
    reasons.push("SMART overall health check failed".to_string());
  }

  if let Some(temp) = signals.temperature_celsius {
    if temp >= TEMPERATURE_CRITICAL_CELSIUS {
      promote_level(&mut level, StorageWarningLevel::Critical);
      reasons.push(format!("Temperature is critical ({temp:.0}°C)"));
    } else if temp >= TEMPERATURE_WARNING_CELSIUS {
      promote_level(&mut level, StorageWarningLevel::Warning);
      reasons.push(format!("Temperature is elevated ({temp:.0}°C)"));
    }
  }

  if let Some(used) = signals.percentage_used {
    if used >= PERCENTAGE_USED_CRITICAL {
      promote_level(&mut level, StorageWarningLevel::Critical);
      reasons.push(format!("NVMe percentage used is critical ({used:.0}%)"));
    } else if used >= PERCENTAGE_USED_WARNING {
      promote_level(&mut level, StorageWarningLevel::Warning);
      reasons.push(format!("NVMe percentage used is high ({used:.0}%)"));
    }
  }

  if let Some(spare) = signals.available_spare_percent {
    if spare <= AVAILABLE_SPARE_CRITICAL_PERCENT {
      promote_level(&mut level, StorageWarningLevel::Critical);
      reasons.push(format!("Available spare is critical ({spare:.0}%)"));
    } else if spare <= AVAILABLE_SPARE_WARNING_PERCENT {
      promote_level(&mut level, StorageWarningLevel::Warning);
      reasons.push(format!("Available spare is low ({spare:.0}%)"));
    }
  }

  if signals.reallocated_sector_count.unwrap_or(0) > 0 {
    promote_level(&mut level, StorageWarningLevel::Warning);
    reasons.push("Reallocated sectors are present".to_string());
  }
  if signals.current_pending_sector_count.unwrap_or(0) > 0 {
    promote_level(&mut level, StorageWarningLevel::Critical);
    reasons.push("Current pending sectors are present".to_string());
  }
  if signals.offline_uncorrectable_count.unwrap_or(0) > 0 {
    promote_level(&mut level, StorageWarningLevel::Critical);
    reasons.push("Offline uncorrectable sectors are present".to_string());
  }
  if signals.media_errors.unwrap_or(0) > 0 {
    promote_level(&mut level, StorageWarningLevel::Critical);
    reasons.push("NVMe media errors are present".to_string());
  }

  let health_status = match level {
    StorageWarningLevel::None => StorageHealthStatus::Good,
    StorageWarningLevel::Warning => StorageHealthStatus::Warning,
    StorageWarningLevel::Critical => StorageHealthStatus::Critical,
    StorageWarningLevel::Unknown => StorageHealthStatus::Unknown,
  };

  (health_status, level, reasons)
}

fn promote_level(current: &mut StorageWarningLevel, next: StorageWarningLevel) {
  let current_rank = warning_rank(current);
  let next_rank = warning_rank(&next);
  if next_rank > current_rank {
    *current = next;
  }
}

fn warning_rank(level: &StorageWarningLevel) -> u8 {
  match level {
    StorageWarningLevel::None => 0,
    StorageWarningLevel::Unknown => 1,
    StorageWarningLevel::Warning => 2,
    StorageWarningLevel::Critical => 3,
  }
}

/// The Storage Health device identity: `storage:hmac-sha256:v1:<64 hex>`,
/// keyed by the user's own identity hash key so a serial number never reaches
/// the database.
///
/// Public, rather than `pub(crate)`, because it is the one value a test may
/// not invent. The archive backends join, upsert and order on this id, and a
/// fixture that seeded a short unprefixed string would certify a backend
/// against an identity shape production never produces - the failure mode
/// `.agents/skills/verify-identity-contracts/SKILL.md` exists to prevent. The
/// differential DuckDB fixture derives every device id through this function
/// for that reason.
pub fn storage_device_id(
  disk: &SmartDiskInfo,
  identity_hash_key: &[u8; STORAGE_HEALTH_IDENTITY_HASH_KEY_BYTES],
) -> String {
  let identity = if let Some(serial) = normalized_serial(disk) {
    format!("{}:{}", disk_protocol(disk), serial)
  } else {
    format!(
      "{}:{}:{}:{:?}",
      disk_protocol(disk),
      disk.model_name.as_deref().unwrap_or("unknown"),
      disk.device_name,
      disk.capacity_bytes,
    )
  };

  format!("storage:{}", hash_identifier(&identity, identity_hash_key))
}

fn normalized_serial(disk: &SmartDiskInfo) -> Option<&str> {
  disk
    .serial_number
    .as_deref()
    .map(str::trim)
    .filter(|serial| !serial.is_empty())
}

fn disk_protocol(disk: &SmartDiskInfo) -> &str {
  disk
    .protocol
    .as_deref()
    .or(disk.device_type.as_deref())
    .unwrap_or("unknown")
}

fn hash_identifier(
  value: &str,
  identity_hash_key: &[u8; STORAGE_HEALTH_IDENTITY_HASH_KEY_BYTES],
) -> String {
  let mut mac = Hmac::<Sha256>::new_from_slice(identity_hash_key)
    .expect("HMAC-SHA-256 accepts any key length");
  mac.update(value.trim().as_bytes());
  let digest = mac.finalize().into_bytes();
  format!("hmac-sha256:v1:{}", hex_encode(&digest))
}

fn hex_encode(bytes: &[u8]) -> String {
  const HEX: &[u8; 16] = b"0123456789abcdef";
  let mut output = String::with_capacity(bytes.len() * 2);
  for byte in bytes {
    output.push(HEX[(byte >> 4) as usize] as char);
    output.push(HEX[(byte & 0x0f) as usize] as char);
  }
  output
}

pub(crate) fn attr_value_u64(
  disk: &SmartDiskInfo,
  id: Option<u32>,
  name: &str,
) -> Option<u64> {
  find_attr(disk, id, name).and_then(|attr| {
    attr
      .raw_value
      .as_deref()
      .and_then(parse_smart_u64)
      .or(attr.current)
  })
}

pub(crate) fn attr_value_f32(
  disk: &SmartDiskInfo,
  id: Option<u32>,
  name: &str,
) -> Option<f32> {
  find_attr(disk, id, name).and_then(|attr| {
    attr
      .raw_value
      .as_deref()
      .and_then(parse_smart_f32)
      .or_else(|| attr.current.map(|v| v as f32))
  })
}

fn find_attr<'a>(
  disk: &'a SmartDiskInfo,
  id: Option<u32>,
  name: &str,
) -> Option<&'a SmartAttribute> {
  match id {
    Some(id) => disk.attributes.iter().find(|attr| attr.id == Some(id)),
    None => disk
      .attributes
      .iter()
      .find(|attr| attr.name.eq_ignore_ascii_case(name)),
  }
}

fn parse_smart_u64(raw: &str) -> Option<u64> {
  raw.split_whitespace().next()?.parse::<u64>().ok()
}

fn parse_smart_f32(raw: &str) -> Option<f32> {
  raw.split_whitespace().next()?.parse::<f32>().ok()
}

fn local_date_string() -> String {
  chrono::Local::now()
    .date_naive()
    .format("%Y-%m-%d")
    .to_string()
}

#[cfg(target_os = "linux")]
fn collect_platform_smart_info_with_guidance() -> SmartInfoCollectionOutcome {
  crate::infrastructure::providers::linux::smart::get_smart_info_with_guidance()
}

#[cfg(target_os = "macos")]
fn collect_platform_smart_info_with_guidance() -> SmartInfoCollectionOutcome {
  crate::infrastructure::providers::macos::smart::get_smart_info_with_guidance()
}

#[cfg(target_os = "windows")]
fn collect_platform_smart_info_with_guidance() -> SmartInfoCollectionOutcome {
  crate::infrastructure::providers::windows::smart::get_smart_info_with_guidance()
}

#[cfg(test)]
mod tests {
  use super::*;

  const TEST_IDENTITY_HASH_KEY: [u8; STORAGE_HEALTH_IDENTITY_HASH_KEY_BYTES] = [0x42; 32];

  fn disk(attributes: Vec<SmartAttribute>) -> SmartDiskInfo {
    SmartDiskInfo {
      device_name: "/dev/sda".to_string(),
      device_type: Some("sat".to_string()),
      protocol: Some("ATA".to_string()),
      model_name: Some("Example SSD".to_string()),
      serial_number: Some("SERIAL123".to_string()),
      firmware_version: None,
      capacity_bytes: Some(512_110_190_592),
      health_status: SmartHealthStatus::Passed,
      temperature_celsius: Some(40),
      power_on_hours: Some(100),
      power_cycle_count: Some(10),
      attributes,
    }
  }

  fn attr(id: Option<u32>, name: &str, raw: &str) -> SmartAttribute {
    SmartAttribute {
      id,
      name: name.to_string(),
      current: None,
      worst: None,
      threshold: None,
      raw_value: Some(raw.to_string()),
      when_failed: None,
    }
  }

  #[test]
  fn active_refresh_ids_include_collected_device_ids() {
    let active_device_ids = active_device_ids_with_collected_devices(
      vec!["live-disk".to_string()],
      &[StorageDeviceRecord {
        id: "daily-disk".to_string(),
        display_name: "Daily Disk".to_string(),
        model: Some("Example SSD".to_string()),
        serial_hash: Some("serial-daily-disk".to_string()),
        protocol: Some("NVMe".to_string()),
        capacity_bytes: Some(1_000_000),
        first_seen_at: "2026-05-10T00:00:00Z".to_string(),
        last_seen_at: "2026-05-10T00:00:00Z".to_string(),
      }],
    );

    assert_eq!(
      active_device_ids,
      vec!["live-disk".to_string(), "daily-disk".to_string()]
    );
  }

  #[test]
  fn empty_active_refresh_ids_preserve_non_authoritative_insert_path() {
    let active_device_ids = active_device_ids_with_collected_devices(
      Vec::new(),
      &[StorageDeviceRecord {
        id: "daily-disk".to_string(),
        display_name: "Daily Disk".to_string(),
        model: Some("Example SSD".to_string()),
        serial_hash: Some("serial-daily-disk".to_string()),
        protocol: Some("NVMe".to_string()),
        capacity_bytes: Some(1_000_000),
        first_seen_at: "2026-05-10T00:00:00Z".to_string(),
        last_seen_at: "2026-05-10T00:00:00Z".to_string(),
      }],
    );

    assert!(active_device_ids.is_empty());
  }

  #[test]
  fn builds_record_fields_from_smart_attributes() {
    let input = disk(vec![
      attr(Some(5), "Reallocated Sector Count", "3"),
      attr(Some(197), "Current Pending Sector Count", "0"),
      attr(Some(198), "Offline Uncorrectable Sector Count", "0"),
    ]);

    let (device, record) = build_daily_record(
      &input,
      "2026-05-10",
      "2026-05-10T00:00:00Z",
      &TEST_IDENTITY_HASH_KEY,
    );

    assert!(device.id.starts_with("storage:"));
    let expected_serial_hash = hash_identifier("SERIAL123", &TEST_IDENTITY_HASH_KEY);
    assert_eq!(
      device.serial_hash.as_deref(),
      Some(expected_serial_hash.as_str())
    );
    assert_eq!(record.device_id, device.id);
    assert_eq!(record.date, "2026-05-10");
    assert_eq!(record.health_status, StorageHealthStatus::Warning);
    assert_eq!(record.warning_level, StorageWarningLevel::Warning);
    assert_eq!(record.reallocated_sector_count, Some(3));
  }

  #[test]
  fn classifies_critical_nvme_signals() {
    let (_, record) = build_daily_record(
      &disk(vec![
        attr(None, "Percentage Used", "101"),
        attr(None, "Available Spare", "5"),
        attr(None, "Media Errors", "1"),
      ]),
      "2026-05-10",
      "2026-05-10T00:00:00Z",
      &TEST_IDENTITY_HASH_KEY,
    );

    assert_eq!(record.health_status, StorageHealthStatus::Critical);
    assert_eq!(record.warning_level, StorageWarningLevel::Critical);
    assert_eq!(record.percentage_used, Some(101.0));
    assert_eq!(record.available_spare_percent, Some(5.0));
    assert_eq!(record.media_errors, Some(1));
    assert!(!record.warning_reasons.is_empty());
  }

  #[test]
  fn stable_device_id_prefers_serial_identity() {
    let mut first = disk(Vec::new());
    first.device_name = "/dev/sda".to_string();
    let mut second = first.clone();
    second.device_name = "/dev/sdb".to_string();

    assert_eq!(
      storage_device_id(&first, &TEST_IDENTITY_HASH_KEY),
      storage_device_id(&second, &TEST_IDENTITY_HASH_KEY)
    );
  }

  #[test]
  fn blank_serial_is_treated_as_missing_identity() {
    let mut first = disk(Vec::new());
    first.serial_number = Some("   ".to_string());
    first.device_name = "/dev/sda".to_string();
    let mut second = first.clone();
    second.device_name = "/dev/sdb".to_string();

    let (device, _) = build_daily_record(
      &first,
      "2026-05-10",
      "2026-05-10T00:00:00Z",
      &TEST_IDENTITY_HASH_KEY,
    );

    assert_eq!(device.serial_hash, None);
    assert_ne!(
      storage_device_id(&first, &TEST_IDENTITY_HASH_KEY),
      storage_device_id(&second, &TEST_IDENTITY_HASH_KEY)
    );
  }

  #[test]
  fn device_id_uses_device_type_when_protocol_is_missing() {
    let mut input = disk(Vec::new());
    input.protocol = None;
    input.device_type = Some("nvme".to_string());

    let expected = format!(
      "storage:{}",
      hash_identifier("nvme:SERIAL123", &TEST_IDENTITY_HASH_KEY)
    );

    assert_eq!(storage_device_id(&input, &TEST_IDENTITY_HASH_KEY), expected);
  }

  #[test]
  fn hashed_identifiers_are_versioned_and_keyed() {
    let first_key = [0x42; STORAGE_HEALTH_IDENTITY_HASH_KEY_BYTES];
    let second_key = [0x24; STORAGE_HEALTH_IDENTITY_HASH_KEY_BYTES];

    let first_hash = hash_identifier("SERIAL123", &first_key);
    let second_hash = hash_identifier("SERIAL123", &second_key);

    assert!(first_hash.starts_with("hmac-sha256:v1:"));
    assert_ne!(first_hash, second_hash);
  }

  #[test]
  fn attr_lookup_uses_id_only_when_id_is_specified() {
    let input = disk(vec![
      attr(Some(9), "Reallocated Sector Count", "99"),
      attr(Some(5), "Vendor Specific", "3"),
    ]);

    assert_eq!(
      attr_value_u64(&input, Some(5), "Reallocated Sector Count"),
      Some(3)
    );
  }

  #[test]
  fn attr_lookup_does_not_fallback_to_name_when_id_is_missing() {
    let input = disk(vec![attr(None, "Reallocated Sector Count", "99")]);

    assert_eq!(
      attr_value_u64(&input, Some(5), "Reallocated Sector Count"),
      None
    );
  }
}
