#![cfg(feature = "duckdb-archive")]
//! The Storage Health family, native beside SQLite.
//!
//! Every test here is a differential one: one fixture is seeded through the
//! real SQLite writers, converted through the real candidate and finalization
//! path, and then the *same question* is put to both engines. A native module
//! is correct here only if it answers what SQLite answers - not if it answers
//! something a reviewer finds reasonable.
//!
//! **Identities are producer-shaped.** Every fixture device id is derived
//! through [`storage_device_id`], the production identity function, so the
//! rows these backends join, upsert and order on carry the real
//! `storage:hmac-sha256:v1:<64 hex>` namespace rather than a short readable
//! string. A fixture that flattened that shape is exactly how a broken join
//! gets certified as correct (`.agents/skills/verify-identity-contracts`), and
//! it would also have hidden the practical consequence here: ids this long
//! share a 23-character prefix, so nothing in this file may identify a test's
//! own rows by a prefix match. Each test names the devices it seeded and every
//! cross-engine read is scoped to exactly those ids.
//!
//! **One process, one SQLite database.** `db::init` is process-wide, so the
//! whole binary shares one SQLite file while each test holds a private copy of
//! the finalized native snapshot. Three things follow. The whole-table read
//! expectation is captured inside the fixture, before any test can run, since
//! a live SQLite read afterwards would be compared against a native database
//! that never saw the intervening writes. Every test that writes to the shared
//! SQLite database and then reads it back holds [`SQLITE`] across both, so a
//! concurrent `refresh_daily_records` - which clears every active flag, not
//! just its own - cannot land between another test's write and its read. And
//! ids allocated to *new* rows cannot match across engines, because SQLite's
//! `sqlite_sequence` is shared by the whole binary while each native copy
//! carries the high-water mark finalization recovered; that is compared as a
//! delta instead, by `a_conflicting_upsert_burns_an_id_in_both_engines`.
//!
//! Every seeded date is derived from the local clock rather than written as a
//! literal, so no fixture here can quietly become a time bomb the way the one
//! PR #2103 repaired did.

mod native_support;

use chrono::{Duration, Local};
use hardviz_core::infrastructure::database::native_database::{
  NativeCancellation, NativeDatabase, NativeDatabaseOptions,
  storage_health as native_storage_health,
};
use hardviz_core::infrastructure::database::{db, storage_health};
use hardviz_core::models::hardware::{
  SmartDiskInfo, SmartHealthStatus, StorageDeviceRecord, StorageHealthRecord,
  StorageHealthRecordDraft, StorageHealthStatus, StorageWarningLevel,
};
use hardviz_core::persistence::storage_health::storage_device_id;
use hardviz_core::settings::STORAGE_HEALTH_IDENTITY_HASH_KEY_BYTES;
use native_support::{NativeFixture, app_native_schema};
use sqlx::Row;
use tokio::sync::{Mutex, OnceCell};

/// Serializes the tests that write to the process-wide SQLite database and
/// then read it back. See the module header: `refresh_daily_records` clears
/// every device's active flag, so without this a concurrent refresh could
/// land between another test's write and the read it compares.
static SQLITE: Mutex<()> = Mutex::const_new(());

/// The fixture's identity hash key. Any key produces production-shaped ids;
/// a fixed one keeps them stable within a run.
const FIXTURE_IDENTITY_HASH_KEY: [u8; STORAGE_HEALTH_IDENTITY_HASH_KEY_BYTES] =
  [0x42; 32];

fn cancel() -> NativeCancellation {
  NativeCancellation::new()
}

/// A local calendar day `offset` days ago, spelled the way every writer here
/// spells one.
fn day(offset: i64) -> String {
  (Local::now().date_naive() - Duration::days(offset))
    .format("%Y-%m-%d")
    .to_string()
}

fn collected(offset: i64) -> String {
  format!("{}T04:05:06Z", day(offset))
}

/// The SMART disk a fixture serial stands for.
///
/// Only the fields [`storage_device_id`] hashes have to be realistic; the
/// serial is what makes each fixture device distinct, exactly as it does in
/// production, so a test namespaces its devices through the value that is
/// hashed rather than through a readable prefix on the result.
fn smart_disk(serial: &str) -> SmartDiskInfo {
  SmartDiskInfo {
    device_name: "/dev/nvme0".to_owned(),
    device_type: Some("nvme".to_owned()),
    protocol: Some("NVMe".to_owned()),
    model_name: Some("HARDVIZ FIXTURE SSD".to_owned()),
    serial_number: Some(serial.to_owned()),
    firmware_version: Some("1.0".to_owned()),
    capacity_bytes: Some(1_000_204_886_016),
    health_status: SmartHealthStatus::Passed,
    temperature_celsius: Some(38),
    power_on_hours: Some(1_234),
    power_cycle_count: Some(56),
    attributes: Vec::new(),
  }
}

/// The device id production would emit for that disk, through the production
/// function rather than through a restatement of its shape.
fn device_id(serial: &str) -> String {
  storage_device_id(&smart_disk(serial), &FIXTURE_IDENTITY_HASH_KEY)
}

fn device_ids(serials: &[&str]) -> Vec<String> {
  serials.iter().map(|serial| device_id(serial)).collect()
}

fn device(serial: &str, display_name: &str) -> StorageDeviceRecord {
  let id = device_id(serial);
  StorageDeviceRecord {
    model: Some("HARDVIZ FIXTURE SSD".to_owned()),
    // The production shape of a hashed serial, derived from the identity
    // rather than invented. Nothing joins on it; the upsert's
    // `COALESCE(excluded.serial_hash, ...)` rule is what the fixture exercises.
    serial_hash: Some(format!("hmac-sha256:v1:{}", &id[id.len() - 64..])),
    protocol: Some("NVMe".to_owned()),
    capacity_bytes: Some(1_000_204_886_016),
    first_seen_at: collected(30),
    last_seen_at: collected(0),
    id,
    display_name: display_name.to_owned(),
  }
}

/// A device whose every optional column is absent, so the NULL path through
/// both readers is exercised rather than assumed.
fn bare_device(serial: &str, display_name: &str) -> StorageDeviceRecord {
  StorageDeviceRecord {
    model: None,
    serial_hash: None,
    protocol: None,
    capacity_bytes: None,
    ..device(serial, display_name)
  }
}

fn record(serial: &str, date_offset: i64) -> StorageHealthRecordDraft {
  StorageHealthRecordDraft {
    device_id: device_id(serial),
    date: day(date_offset),
    health_status: StorageHealthStatus::Good,
    warning_level: StorageWarningLevel::None,
    temperature_celsius: Some(38.5),
    power_on_hours: Some(1_234),
    percentage_used: Some(3.0),
    available_spare_percent: Some(100.0),
    reallocated_sector_count: Some(0),
    current_pending_sector_count: Some(0),
    offline_uncorrectable_count: Some(0),
    media_errors: Some(0),
    error_log_entries: Some(7),
    unsafe_shutdown_count: Some(2),
    warning_reasons: vec!["nothing to report".to_owned()],
    collected_at: collected(date_offset),
  }
}

/// A record carrying nothing but the two required enum columns.
fn bare_record(serial: &str, date_offset: i64) -> StorageHealthRecordDraft {
  StorageHealthRecordDraft {
    health_status: StorageHealthStatus::Unknown,
    warning_level: StorageWarningLevel::Unknown,
    temperature_celsius: None,
    power_on_hours: None,
    percentage_used: None,
    available_spare_percent: None,
    reallocated_sector_count: None,
    current_pending_sector_count: None,
    offline_uncorrectable_count: None,
    media_errors: None,
    error_log_entries: None,
    unsafe_shutdown_count: None,
    warning_reasons: Vec::new(),
    ..record(serial, date_offset)
  }
}

/// The devices the `latest_records` comparison reads, and the display names
/// the ordering has to get right. The four accented names are the ones where
/// SQLite's `NOCASE` and DuckDB's collation of the same name disagree.
const READ_DEVICES: &[(&str, &str)] = &[
  ("READ-UPPER", "Disk ABC"),
  ("READ-LOWER", "disk abd"),
  ("READ-ACUTE-UPPER", "Ábc disk"),
  ("READ-DIAERESIS-UPPER", "Älpha disk"),
  ("READ-ACUTE-LOWER", "ábc disk"),
  ("READ-DIAERESIS-LOWER", "älpha disk"),
  ("READ-INACTIVE", "Retired disk"),
];

/// Every serial the seed leaves active. `READ-INACTIVE` is deliberately not
/// here: it is switched off through the production refresh path so the
/// `is_active` filter has a device to exclude.
const ACTIVE_AT_SEED: &[&str] = &[
  "READ-UPPER",
  "READ-LOWER",
  "READ-ACUTE-UPPER",
  "READ-DIAERESIS-UPPER",
  "READ-ACUTE-LOWER",
  "READ-DIAERESIS-LOWER",
  "READ-BARE",
  "READ-CRITICAL",
  "READ-WARNING",
  "UPSERT-A",
  "REFRESH-A",
  "REFRESH-B",
  "RET-A",
];

/// The seeded SQLite source, the finalized template copied from it, and the
/// SQLite answer captured at the same instant the template was taken.
struct Shared {
  fixture: NativeFixture,
  latest_at_seed: Vec<StorageHealthRecord>,
}

static SHARED: OnceCell<Shared> = OnceCell::const_new();

async fn shared() -> &'static Shared {
  SHARED
    .get_or_init(|| async {
      let fixture = NativeFixture::new();
      assert!(db::init(fixture.source.clone()));
      let pool = fixture.migrated_pool().await;
      seed().await;
      // Captured here, not in the test: the native side is the snapshot this
      // line stands beside, and a live read later would be compared against a
      // database that never saw the intervening writes.
      let latest_at_seed = storage_health::latest_records().await.unwrap();
      pool.close().await;

      fixture.finalize().await;
      Shared {
        fixture,
        latest_at_seed,
      }
    })
    .await
}

/// A fixture with deliberately awkward shape:
///
/// - four devices whose display names differ only by a non-ASCII letter's
///   case, which is exactly where SQLite's `NOCASE` and DuckDB's disagree;
/// - two devices whose names differ only by ASCII case, where they agree;
/// - several dates per device, and several devices sharing one maximum date,
///   so the `MAX(date)` join has ties to resolve;
/// - one device switched off, so the `is_active` filter has work to do;
/// - one device and one record with every optional column absent;
/// - every warning level, so the severity ordering is exercised.
async fn seed() {
  for (serial, display_name) in READ_DEVICES {
    // Three days each, so the per-device MAX(date) picks one of several.
    for offset in [5i64, 3, 1] {
      storage_health::insert_daily_records(
        vec![device(serial, display_name)],
        vec![record(serial, offset)],
      )
      .await
      .unwrap();
    }
  }

  storage_health::insert_daily_records(
    vec![bare_device("READ-BARE", "Bare disk")],
    vec![bare_record("READ-BARE", 1)],
  )
  .await
  .unwrap();

  for (serial, display_name, status, level, reasons) in [
    (
      "READ-CRITICAL",
      "Failing disk",
      StorageHealthStatus::Critical,
      StorageWarningLevel::Critical,
      vec!["Current pending sectors are present".to_owned()],
    ),
    (
      "READ-WARNING",
      "Ageing disk",
      StorageHealthStatus::Warning,
      StorageWarningLevel::Warning,
      vec!["Spare capacity is low".to_owned()],
    ),
  ] {
    storage_health::insert_daily_records(
      vec![device(serial, display_name)],
      vec![StorageHealthRecordDraft {
        health_status: status,
        warning_level: level,
        warning_reasons: reasons,
        ..record(serial, 1)
      }],
    )
    .await
    .unwrap();
  }

  // The namespaces the mutating tests own, seeded so each has a baseline both
  // engines start from.
  storage_health::insert_daily_records(
    vec![device("UPSERT-A", "Upsert disk")],
    vec![record("UPSERT-A", 2)],
  )
  .await
  .unwrap();
  storage_health::insert_daily_records(
    vec![
      device("REFRESH-A", "Refresh disk A"),
      device("REFRESH-B", "Refresh disk B"),
    ],
    vec![record("REFRESH-A", 2), record("REFRESH-B", 2)],
  )
  .await
  .unwrap();
  // Two rows far older than any retention this test asks for, and two well
  // inside it - all relative to today, so the fixture cannot expire.
  for offset in [400i64, 200, 10, 2] {
    storage_health::insert_daily_records(
      vec![device("RET-A", "Retained disk")],
      vec![record("RET-A", offset)],
    )
    .await
    .unwrap();
  }

  // Switch `READ-INACTIVE` off through the production path rather than by
  // hand, so the flag the reader filters on was written the way the
  // application writes it.
  storage_health::refresh_daily_records(
    &device_ids(ACTIVE_AT_SEED),
    Vec::new(),
    Vec::new(),
  )
  .await
  .unwrap();
}

/// A private copy of the finalized fixture, plus the owner serving it.
///
/// Every test takes its own copy instead of sharing one open `NativeDatabase`.
/// On Windows a DuckDB file cannot be read, hashed, or opened by a second
/// instance while any owner still holds it, so a shared open handle makes
/// `std::fs::copy` fail with a sharing violation and `read_only` fail with
/// "File is already open".
struct Owned {
  _directory: tempfile::TempDir,
  path: std::path::PathBuf,
  database: Option<NativeDatabase>,
}

impl Owned {
  async fn open(name: &str) -> Self {
    let shared = shared().await;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(name);
    std::fs::copy(&shared.fixture.finalized, &path).unwrap();
    let database = NativeDatabase::open(
      &path,
      NativeDatabaseOptions::new(app_native_schema::NATIVE_SCHEMA_VERSION),
    )
    .await
    .unwrap();
    Self {
      _directory: directory,
      path,
      database: Some(database),
    }
  }

  fn database(&self) -> &NativeDatabase {
    self.database.as_ref().expect("database already closed")
  }

  /// Release the file so a second instance may open it. Call before any
  /// `read_only` on [`Owned::path`].
  async fn close(&mut self) {
    if let Some(database) = self.database.take() {
      database.close().await.unwrap();
    }
  }
}

/// Every column of one `storage_devices` row, in schema order.
#[derive(Debug, PartialEq, Eq)]
struct DeviceRow {
  id: String,
  display_name: String,
  model: Option<String>,
  serial_hash: Option<String>,
  protocol: Option<String>,
  capacity_bytes: Option<i64>,
  first_seen_at: String,
  last_seen_at: String,
  is_active: i64,
}

const DEVICE_COLUMNS: &str = "id, display_name, model, serial_hash, protocol, \
   capacity_bytes, first_seen_at, last_seen_at, is_active";

/// Every column of one `storage_health_daily_records` row, in schema order.
#[derive(Debug, PartialEq)]
struct RecordRow {
  id: i64,
  device_id: String,
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

/// The same rows with their `id` dropped.
///
/// A row *inserted* during a test cannot have the same id on both sides: the
/// native copy carries the identity high-water mark finalization recovered
/// from the fixture, while the shared SQLite database's `sqlite_sequence` has
/// been advanced by every other test in this binary. That is a property of the
/// one-database-per-process fixture, not of the writers - the ids they hand
/// out are compared where it is meaningful, as a delta, by
/// `a_conflicting_upsert_burns_an_id_in_both_engines`, and absolutely by the
/// upsert test, whose row is updated in place and keeps the id the seed gave
/// it.
fn forget_ids(rows: Vec<RecordRow>) -> Vec<RecordRow> {
  rows
    .into_iter()
    .map(|row| RecordRow { id: 0, ..row })
    .collect()
}

const RECORD_COLUMNS: &str = "id, device_id, date, health_status, warning_level, \
   warning_reasons, temperature_celsius, power_on_hours, percentage_used, \
   available_spare_percent, reallocated_sector_count, \
   current_pending_sector_count, offline_uncorrectable_count, media_errors, \
   error_log_entries, unsafe_shutdown_count, collected_at";

/// `<column> IN (?, ?, ...)`.
///
/// Production ids are 87 characters that share a 23-character prefix, so a
/// `LIKE` filter can no longer separate one test's devices from another's.
/// Every cross-engine read names the ids it seeded instead.
fn in_clause(column: &str, count: usize) -> String {
  let placeholders = vec!["?"; count].join(", ");
  format!("{column} IN ({placeholders})")
}

async fn sqlite_devices(serials: &[&str]) -> Vec<DeviceRow> {
  let shared = shared().await;
  let ids = device_ids(serials);
  let pool = native_support::open_pool(&shared.fixture.source, false).await;
  let sql = format!(
    "SELECT {DEVICE_COLUMNS} FROM storage_devices WHERE {} ORDER BY id",
    in_clause("id", ids.len())
  );
  let mut query = sqlx::query(&sql);
  for id in &ids {
    query = query.bind(id);
  }
  let rows = query.fetch_all(&pool).await.unwrap();
  pool.close().await;
  rows
    .into_iter()
    .map(|row| DeviceRow {
      id: row.get(0),
      display_name: row.get(1),
      model: row.get(2),
      serial_hash: row.get(3),
      protocol: row.get(4),
      capacity_bytes: row.get(5),
      first_seen_at: row.get(6),
      last_seen_at: row.get(7),
      is_active: row.get(8),
    })
    .collect()
}

fn decode_device_row(row: &duckdb::Row<'_>) -> duckdb::Result<DeviceRow> {
  Ok(DeviceRow {
    id: row.get(0)?,
    display_name: row.get(1)?,
    model: row.get(2)?,
    serial_hash: row.get(3)?,
    protocol: row.get(4)?,
    capacity_bytes: row.get(5)?,
    first_seen_at: row.get(6)?,
    last_seen_at: row.get(7)?,
    is_active: row.get(8)?,
  })
}

fn native_devices(path: &std::path::Path, serials: &[&str]) -> Vec<DeviceRow> {
  let ids = device_ids(serials);
  let connection = native_support::read_only(path);
  let mut statement = connection
    .prepare(&format!(
      "SELECT {DEVICE_COLUMNS} FROM storage_devices WHERE {} ORDER BY id",
      in_clause("id", ids.len())
    ))
    .unwrap();
  statement
    .query_map(duckdb::params_from_iter(ids.iter()), decode_device_row)
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

/// [`native_devices`] through the owner, for a database still open.
async fn native_devices_open(
  database: &NativeDatabase,
  serials: &[&str],
) -> Vec<DeviceRow> {
  let ids = device_ids(serials);
  let sql = format!(
    "SELECT {DEVICE_COLUMNS} FROM storage_devices WHERE {} ORDER BY id",
    in_clause("id", ids.len())
  );
  database
    .request_read(cancel(), move |context| {
      let mut statement = context.connection().prepare(&sql).unwrap();
      Ok(
        statement
          .query_map(duckdb::params_from_iter(ids.iter()), decode_device_row)
          .unwrap()
          .collect::<Result<Vec<_>, _>>()
          .unwrap(),
      )
    })
    .await
    .unwrap()
}

async fn sqlite_records(serials: &[&str]) -> Vec<RecordRow> {
  let shared = shared().await;
  let ids = device_ids(serials);
  let pool = native_support::open_pool(&shared.fixture.source, false).await;
  let sql = format!(
    "SELECT {RECORD_COLUMNS} FROM storage_health_daily_records
     WHERE {} ORDER BY device_id, date",
    in_clause("device_id", ids.len())
  );
  let mut query = sqlx::query(&sql);
  for id in &ids {
    query = query.bind(id);
  }
  let rows = query.fetch_all(&pool).await.unwrap();
  pool.close().await;
  rows
    .into_iter()
    .map(|row| RecordRow {
      id: row.get(0),
      device_id: row.get(1),
      date: row.get(2),
      health_status: row.get(3),
      warning_level: row.get(4),
      warning_reasons: row.get(5),
      temperature_celsius: row.get(6),
      power_on_hours: row.get(7),
      percentage_used: row.get(8),
      available_spare_percent: row.get(9),
      reallocated_sector_count: row.get(10),
      current_pending_sector_count: row.get(11),
      offline_uncorrectable_count: row.get(12),
      media_errors: row.get(13),
      error_log_entries: row.get(14),
      unsafe_shutdown_count: row.get(15),
      collected_at: row.get(16),
    })
    .collect()
}

fn native_records(path: &std::path::Path, serials: &[&str]) -> Vec<RecordRow> {
  let ids = device_ids(serials);
  let connection = native_support::read_only(path);
  let mut statement = connection
    .prepare(&format!(
      "SELECT {RECORD_COLUMNS} FROM storage_health_daily_records
       WHERE {} ORDER BY device_id, date",
      in_clause("device_id", ids.len())
    ))
    .unwrap();
  statement
    .query_map(duckdb::params_from_iter(ids.iter()), |row| {
      Ok(RecordRow {
        id: row.get(0)?,
        device_id: row.get(1)?,
        date: row.get(2)?,
        health_status: row.get(3)?,
        warning_level: row.get(4)?,
        warning_reasons: row.get(5)?,
        temperature_celsius: row.get(6)?,
        power_on_hours: row.get(7)?,
        percentage_used: row.get(8)?,
        available_spare_percent: row.get(9)?,
        reallocated_sector_count: row.get(10)?,
        current_pending_sector_count: row.get(11)?,
        offline_uncorrectable_count: row.get(12)?,
        media_errors: row.get(13)?,
        error_log_entries: row.get(14)?,
        unsafe_shutdown_count: row.get(15)?,
        collected_at: row.get(16)?,
      })
    })
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

/// How many daily records the still-open native database holds for `serials`.
async fn native_record_count_open(database: &NativeDatabase, serials: &[&str]) -> i64 {
  let ids = device_ids(serials);
  let sql = format!(
    "SELECT COUNT(*) FROM storage_health_daily_records WHERE {}",
    in_clause("device_id", ids.len())
  );
  database
    .request_read(cancel(), move |context| {
      Ok(
        context
          .connection()
          .query_row(&sql, duckdb::params_from_iter(ids.iter()), |row| {
            row.get::<_, i64>(0)
          })
          .unwrap(),
      )
    })
    .await
    .unwrap()
}

/// The native allocator's high-water mark for the daily records table, which
/// is the value finalization recovered from `sqlite_sequence`.
async fn native_high_water(database: &NativeDatabase) -> i64 {
  database
    .request_read(cancel(), |context| {
      Ok(
        context
          .connection()
          .query_row(
            "SELECT high_water FROM __hv_native_identities \
             WHERE table_name = 'storage_health_daily_records'",
            [],
            |row| row.get::<_, i64>(0),
          )
          .unwrap(),
      )
    })
    .await
    .unwrap()
}

/// The SQLite answer the native reader has to reproduce, captured before any
/// test could write to the shared database.
#[tokio::test]
async fn latest_records_answers_what_sqlite_answers() {
  let shared = shared().await;
  let mut owned = Owned::open("latest.duckdb").await;

  let actual = native_storage_health::latest_records(owned.database(), cancel())
    .await
    .unwrap();
  assert_eq!(actual, shared.latest_at_seed);

  // The comparison is only evidence if the fixture produced the shapes the
  // query has to get right.
  let expected = &shared.latest_at_seed;
  assert_eq!(
    expected.len(),
    ACTIVE_AT_SEED.len(),
    "one row per active device and none for the retired one"
  );
  assert!(
    !expected
      .iter()
      .any(|row| row.device_id == device_id("READ-INACTIVE")),
    "the inactive device must be filtered out"
  );
  assert!(
    expected.iter().all(|row| {
      // 23 characters of shared namespace and 64 of hex - the shape the
      // scoping rule above is a consequence of, pinned rather than asserted
      // in a comment.
      row.device_id.starts_with("storage:hmac-sha256:v1:") && row.device_id.len() == 87
    }),
    "the fixture carries production-shaped identities"
  );
  assert_eq!(expected[0].device_id, device_id("READ-CRITICAL"));
  assert_eq!(expected[1].device_id, device_id("READ-WARNING"));
  let read_ids = device_ids(&[
    "READ-UPPER",
    "READ-LOWER",
    "READ-ACUTE-UPPER",
    "READ-DIAERESIS-UPPER",
    "READ-ACUTE-LOWER",
    "READ-DIAERESIS-LOWER",
    "READ-BARE",
    "READ-CRITICAL",
    "READ-WARNING",
  ]);
  assert!(
    expected
      .iter()
      .filter(|row| read_ids.contains(&row.device_id))
      .all(|row| row.date == day(1)),
    "each device's newest day wins, and several devices tie on it"
  );
  let bare = expected
    .iter()
    .find(|row| row.device_id == device_id("READ-BARE"))
    .unwrap();
  assert_eq!(bare.model, None);
  assert_eq!(bare.capacity_bytes, None);
  assert_eq!(bare.temperature_celsius, None);
  assert_eq!(bare.warning_reasons, Vec::<String>::new());
  assert_eq!(bare.health_status, StorageHealthStatus::Unknown);

  // The ordering claim, stated as the order the accented names actually came
  // back in: SQLite folds only ASCII, so `Á` sorts before `Ä` sorts before
  // `á` sorts before `ä`. DuckDB's own `COLLATE NOCASE` would have paired the
  // cases instead - the divergence the native query avoids.
  let accented_ids = device_ids(&[
    "READ-ACUTE-UPPER",
    "READ-DIAERESIS-UPPER",
    "READ-ACUTE-LOWER",
    "READ-DIAERESIS-LOWER",
  ]);
  let accented: Vec<&str> = expected
    .iter()
    .filter(|row| accented_ids.contains(&row.device_id))
    .map(|row| row.display_name.as_str())
    .collect();
  assert_eq!(
    accented,
    vec!["Ábc disk", "Älpha disk", "ábc disk", "älpha disk"]
  );
  owned.close().await;
}

/// SQLite's `NOCASE` and DuckDB's collation of the same name are not the same
/// ordering, so the native query cannot simply borrow the spelling. Both are
/// asked directly here, over the display names the fixture carries, so the
/// choice in the native module is backed by a measurement rather than by a
/// reading of two manuals.
///
/// All three queries are scoped to the devices this fixture seeded for the
/// purpose: a whole-table read would let any other test's rows into the
/// oracle while leaving the finalized native copy without them.
#[tokio::test]
async fn duckdb_collate_nocase_is_not_sqlites_and_the_translate_key_is() {
  let shared = shared().await;
  let mut owned = Owned::open("collation.duckdb").await;

  let serials: Vec<&str> = READ_DEVICES.iter().map(|(serial, _)| *serial).collect();
  let ids = device_ids(&serials);
  let scope = in_clause("id", ids.len());

  let pool = native_support::open_pool(&shared.fixture.source, false).await;
  let sql = format!(
    "SELECT display_name FROM storage_devices WHERE {scope} \
     ORDER BY display_name COLLATE NOCASE"
  );
  let mut query = sqlx::query_scalar(&sql);
  for id in &ids {
    query = query.bind(id);
  }
  let oracle: Vec<String> = query.fetch_all(&pool).await.unwrap();
  pool.close().await;

  owned.close().await;
  let connection = native_support::read_only(&owned.path);
  let order = |sql: &str| -> Vec<String> {
    let mut statement = connection.prepare(sql).unwrap();
    statement
      .query_map(duckdb::params_from_iter(ids.iter()), |row| row.get(0))
      .unwrap()
      .collect::<Result<_, _>>()
      .unwrap()
  };
  let duckdb_nocase = order(&format!(
    "SELECT display_name FROM storage_devices WHERE {scope} \
     ORDER BY display_name COLLATE NOCASE"
  ));
  let translated = order(&format!(
    "SELECT display_name FROM storage_devices WHERE {scope} ORDER BY \
     translate(display_name, 'ABCDEFGHIJKLMNOPQRSTUVWXYZ', 'abcdefghijklmnopqrstuvwxyz')"
  ));

  assert_eq!(
    oracle.len(),
    READ_DEVICES.len(),
    "the scoped oracle saw every seeded name and nothing else"
  );
  assert_eq!(
    translated, oracle,
    "the ASCII fold reproduces SQLite's NOCASE order"
  );
  assert_ne!(
    duckdb_nocase, oracle,
    "DuckDB's own NOCASE does not, which is why it is not used"
  );
}

/// Re-inserting the same device and the same `(device_id, date)` must leave
/// both engines holding the same row, column for column - including the one
/// column the conflict clause does not overwrite.
#[tokio::test]
async fn upsert_parity_over_every_column() {
  let mut owned = Owned::open("upsert.duckdb").await;
  let _sqlite = SQLITE.lock().await;

  let before_sqlite = sqlite_records(&["UPSERT-A"]).await;
  assert_eq!(before_sqlite.len(), 1);

  // A second collection of the same day: fuller readings, a later
  // `last_seen_at`, and a collector that could not read the serial this time.
  let updated_device = StorageDeviceRecord {
    display_name: "Upsert disk (renamed)".to_owned(),
    model: Some("HARDVIZ FIXTURE SSD rev B".to_owned()),
    serial_hash: None,
    capacity_bytes: Some(2_000_409_772_032),
    last_seen_at: collected(0),
    ..device("UPSERT-A", "Upsert disk")
  };
  let updated_record = StorageHealthRecordDraft {
    health_status: StorageHealthStatus::Warning,
    warning_level: StorageWarningLevel::Warning,
    temperature_celsius: Some(51.25),
    percentage_used: Some(11.5),
    warning_reasons: vec!["Temperature is high".to_owned()],
    collected_at: collected(0),
    ..record("UPSERT-A", 2)
  };

  storage_health::insert_daily_records(
    vec![updated_device.clone()],
    vec![updated_record.clone()],
  )
  .await
  .unwrap();
  native_storage_health::insert_daily_records(
    owned.database(),
    cancel(),
    vec![updated_device],
    vec![updated_record],
  )
  .await
  .unwrap();
  owned.close().await;

  let sqlite_devices = sqlite_devices(&["UPSERT-A"]).await;
  let sqlite_records = sqlite_records(&["UPSERT-A"]).await;
  assert_eq!(native_devices(&owned.path, &["UPSERT-A"]), sqlite_devices);
  assert_eq!(native_records(&owned.path, &["UPSERT-A"]), sqlite_records);

  // And the upsert did what the SQL says it does, rather than both engines
  // agreeing on nothing having happened.
  assert_eq!(
    sqlite_records.len(),
    1,
    "the date did not gain a second row"
  );
  assert_eq!(
    sqlite_records[0].id, before_sqlite[0].id,
    "the row was updated in place"
  );
  assert_eq!(sqlite_records[0].health_status, "warning");
  assert_eq!(sqlite_records[0].temperature_celsius, Some(51.25));
  assert_eq!(sqlite_devices[0].display_name, "Upsert disk (renamed)");
  assert_eq!(
    sqlite_devices[0].serial_hash,
    device("UPSERT-A", "Upsert disk").serial_hash,
    "an unreadable serial must not erase the one a privileged run established"
  );
}

/// SQLite's `AUTOINCREMENT` sequence advances even when the upsert lands on
/// `DO UPDATE`, so the id it would have given the discarded row is burned.
/// The native allocator has to burn it too, or the two archives would start
/// handing out different ids. Compared as a delta rather than as an absolute,
/// because every other test in this binary also writes to the shared SQLite
/// database.
#[tokio::test]
async fn a_conflicting_upsert_burns_an_id_in_both_engines() {
  let shared = shared().await;
  let mut owned = Owned::open("identity.duckdb").await;
  let _sqlite = SQLITE.lock().await;

  let sqlite_sequence = || async {
    let pool = native_support::open_pool(&shared.fixture.source, false).await;
    let seq: i64 = sqlx::query_scalar(
      "SELECT seq FROM sqlite_sequence WHERE name = 'storage_health_daily_records'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    pool.close().await;
    seq
  };

  let sqlite_before = sqlite_sequence().await;
  let native_before = native_high_water(owned.database()).await;

  // `(UPSERT-A, day(2))` already exists in both, so this lands on DO UPDATE.
  let conflicting = record("UPSERT-A", 2);
  storage_health::insert_daily_records(
    vec![device("UPSERT-A", "Upsert disk")],
    vec![conflicting.clone()],
  )
  .await
  .unwrap();
  native_storage_health::insert_daily_records(
    owned.database(),
    cancel(),
    vec![device("UPSERT-A", "Upsert disk")],
    vec![conflicting],
  )
  .await
  .unwrap();

  let sqlite_delta = sqlite_sequence().await - sqlite_before;
  let native_delta = native_high_water(owned.database()).await - native_before;
  assert_eq!(
    sqlite_delta, 1,
    "SQLite burns the id the conflict discarded"
  );
  assert_eq!(native_delta, sqlite_delta);
  owned.close().await;
}

/// The active-flag reconciliation, including a device that drops out of the
/// enumeration and one that comes back.
#[tokio::test]
async fn refresh_active_flag_parity() {
  let mut owned = Owned::open("refresh.duckdb").await;
  let _sqlite = SQLITE.lock().await;

  let refresh = |serials: Vec<&'static str>,
                 devices: Vec<StorageDeviceRecord>,
                 records: Vec<StorageHealthRecordDraft>| {
    let ids = device_ids(&serials);
    async move {
      storage_health::refresh_daily_records(&ids, devices.clone(), records.clone())
        .await
        .unwrap();
      (ids, devices, records)
    }
  };

  // `REFRESH-B` drops out of the enumeration.
  let (ids, devices, records) = refresh(vec!["REFRESH-A"], Vec::new(), Vec::new()).await;
  native_storage_health::refresh_daily_records(
    owned.database(),
    cancel(),
    ids,
    devices,
    records,
  )
  .await
  .unwrap();

  // An empty enumeration is uncertainty, not evidence that every disk went
  // away, so nothing may change.
  let (ids, devices, records) = refresh(Vec::new(), Vec::new(), Vec::new()).await;
  native_storage_health::refresh_daily_records(
    owned.database(),
    cancel(),
    ids,
    devices,
    records,
  )
  .await
  .unwrap();

  // `REFRESH-B` comes back, with a new day's record beside it.
  let (ids, devices, records) = refresh(
    vec!["REFRESH-A", "REFRESH-B"],
    vec![device("REFRESH-B", "Refresh disk B")],
    vec![record("REFRESH-B", 0)],
  )
  .await;
  native_storage_health::refresh_daily_records(
    owned.database(),
    cancel(),
    ids,
    devices,
    records,
  )
  .await
  .unwrap();
  owned.close().await;

  let serials = ["REFRESH-A", "REFRESH-B"];
  let sqlite_devices = sqlite_devices(&serials).await;
  assert_eq!(native_devices(&owned.path, &serials), sqlite_devices);
  assert_eq!(
    forget_ids(native_records(&owned.path, &serials)),
    forget_ids(sqlite_records(&serials).await)
  );
  assert_eq!(
    sqlite_devices
      .iter()
      .map(|row| row.is_active)
      .collect::<Vec<_>>(),
    vec![1, 1],
    "both are active again after the disappearing device reappeared"
  );
  assert_eq!(sqlite_devices.len(), 2);
}

/// The same `retention_days` through both paths has to take the same rows and
/// leave `storage_devices` alone. The expected survivors are whatever SQLite
/// actually kept, not a count written into the test.
#[tokio::test]
async fn retention_parity() {
  let mut owned = Owned::open("retention.duckdb").await;
  let _sqlite = SQLITE.lock().await;

  let before_sqlite = sqlite_records(&["RET-A"]).await;
  // Through the owner, not `read_only`: on Windows the file cannot be opened
  // by a second instance while this one still holds it.
  let before_native = native_record_count_open(owned.database(), &["RET-A"]).await;
  assert_eq!(before_native, before_sqlite.len() as i64);
  assert_eq!(before_sqlite.len(), 4);
  // "Retention leaves `storage_devices` alone" is a claim about each engine's
  // own before/after, not about the two agreeing: `refresh_active_flag_parity`
  // may already have cleared this device's active flag in the shared SQLite
  // database without touching this private native copy.
  let devices_before_sqlite = sqlite_devices(&["RET-A"]).await;
  let devices_before_native = native_devices_open(owned.database(), &["RET-A"]).await;
  assert!(!devices_before_sqlite.is_empty());

  storage_health::delete_old_data(30).await.unwrap();
  native_storage_health::delete_old_data(owned.database(), cancel(), 30)
    .await
    .unwrap();
  owned.close().await;

  let after_sqlite = sqlite_records(&["RET-A"]).await;
  assert_eq!(native_records(&owned.path, &["RET-A"]), after_sqlite);
  assert_eq!(
    after_sqlite
      .iter()
      .map(|row| row.date.clone())
      .collect::<Vec<_>>(),
    vec![day(10), day(2)],
    "the two rows inside the window survive and the two ancient ones go"
  );
  assert_eq!(
    sqlite_devices(&["RET-A"]).await,
    devices_before_sqlite,
    "SQLite retention never touches storage_devices"
  );
  assert_eq!(
    native_devices(&owned.path, &["RET-A"]),
    devices_before_native,
    "and neither does the native one"
  );
}

/// A transaction whose last statement fails must leave both tables as they
/// were, in both engines. The failing statement is a record whose `device_id`
/// no `storage_devices` row carries, which the foreign key refuses - the one
/// failure a caller can actually provoke through the public writer.
#[tokio::test]
async fn a_failed_record_rolls_back_the_devices_beside_it() {
  let mut owned = Owned::open("rollback.duckdb").await;
  let _sqlite = SQLITE.lock().await;

  let devices = vec![device("ROLLBACK-NEW", "Rollback disk")];
  let records = vec![
    record("ROLLBACK-NEW", 1),
    // Last, and unsatisfiable: nothing ever wrote this device.
    record("ROLLBACK-GHOST", 1),
  ];

  let sqlite_error =
    storage_health::insert_daily_records(devices.clone(), records.clone())
      .await
      .unwrap_err();
  assert!(
    matches!(&sqlite_error, sqlx::Error::Database(error)
      if error.message().to_ascii_uppercase().contains("FOREIGN KEY")),
    "{sqlite_error:?}"
  );
  let native_error = native_storage_health::insert_daily_records(
    owned.database(),
    cancel(),
    devices,
    records,
  )
  .await
  .unwrap_err();
  assert!(
    format!("{native_error}")
      .to_ascii_lowercase()
      .contains("key"),
    "{native_error:?}"
  );
  owned.close().await;

  let serials = ["ROLLBACK-NEW", "ROLLBACK-GHOST"];
  assert!(sqlite_devices(&serials).await.is_empty());
  assert!(sqlite_records(&serials).await.is_empty());
  assert!(native_devices(&owned.path, &serials).is_empty());
  assert!(native_records(&owned.path, &serials).is_empty());
}

/// SQLite has no NaN: a bound one is stored as NULL, so a NaN reading and an
/// absent one are the same row. The native writer has to reach the same
/// nullness, which is checked as stored nullness on both sides - `typeof` in
/// SQLite, an explicit `isnan` branch natively - rather than only through the
/// decoded value, where a stored NaN would read back as `Some(NaN)` and could
/// be mistaken for a reading.
#[tokio::test]
async fn nan_and_null_readings_are_the_same_absence_in_both_engines() {
  let mut owned = Owned::open("nan.duckdb").await;
  let _sqlite = SQLITE.lock().await;

  let devices = vec![device("NAN-A", "NaN disk"), device("NAN-B", "Null disk")];
  let records = vec![
    StorageHealthRecordDraft {
      temperature_celsius: Some(f32::NAN),
      percentage_used: Some(f32::NAN),
      available_spare_percent: Some(f32::INFINITY),
      ..record("NAN-A", 1)
    },
    StorageHealthRecordDraft {
      temperature_celsius: None,
      percentage_used: None,
      available_spare_percent: None,
      ..record("NAN-B", 1)
    },
  ];

  storage_health::insert_daily_records(devices.clone(), records.clone())
    .await
    .unwrap();
  native_storage_health::insert_daily_records(
    owned.database(),
    cancel(),
    devices,
    records,
  )
  .await
  .unwrap();
  owned.close().await;

  let serials = ["NAN-A", "NAN-B"];
  let ids = device_ids(&serials);
  let scope = in_clause("device_id", ids.len());
  let shared = shared().await;
  let pool = native_support::open_pool(&shared.fixture.source, false).await;
  let sql = format!(
    "SELECT device_id, typeof(temperature_celsius), typeof(percentage_used), \
            typeof(available_spare_percent)
     FROM storage_health_daily_records WHERE {scope} ORDER BY device_id"
  );
  let mut query = sqlx::query(&sql);
  for id in &ids {
    query = query.bind(id);
  }
  let sqlite_classes: Vec<(String, String, String, String)> = query
    .fetch_all(&pool)
    .await
    .unwrap()
    .into_iter()
    .map(|row| (row.get(0), row.get(1), row.get(2), row.get(3)))
    .collect();
  pool.close().await;

  let mut expected: Vec<(String, String, String, String)> = vec![
    (
      device_id("NAN-A"),
      "null".to_owned(),
      "null".to_owned(),
      "real".to_owned(),
    ),
    (
      device_id("NAN-B"),
      "null".to_owned(),
      "null".to_owned(),
      "null".to_owned(),
    ),
  ];
  expected.sort_by(|left, right| left.0.cmp(&right.0));
  assert_eq!(
    sqlite_classes, expected,
    "a bound NaN is stored as NULL; an infinity stays a real"
  );

  let connection = native_support::read_only(&owned.path);
  let class = |column: &str| {
    format!(
      "CASE WHEN {column} IS NULL THEN 'null' \
            WHEN isnan({column}) THEN 'nan' ELSE 'real' END"
    )
  };
  let mut statement = connection
    .prepare(&format!(
      "SELECT device_id, {}, {}, {} FROM storage_health_daily_records \
       WHERE {scope} ORDER BY device_id",
      class("temperature_celsius"),
      class("percentage_used"),
      class("available_spare_percent")
    ))
    .unwrap();
  let native_classes: Vec<(String, String, String, String)> = statement
    .query_map(duckdb::params_from_iter(ids.iter()), |row| {
      Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    })
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap();
  assert_eq!(native_classes, sqlite_classes);

  // And the whole rows agree, so the NaN column is not the only thing that
  // survived the comparison.
  assert_eq!(
    forget_ids(native_records(&owned.path, &serials)),
    forget_ids(sqlite_records(&serials).await)
  );
}

/// A finalized database with nothing in it answers with nothing, rather than
/// failing on the empty `MAX(date)` sub-select.
#[tokio::test]
async fn an_empty_database_answers_with_no_records() {
  // Its own fixture: the shared one is deliberately full, and this is the one
  // question it cannot answer. Migrated through App's own migrations so the
  // tables exist and are simply empty.
  let fixture = NativeFixture::new();
  let pool = fixture.migrated_pool().await;
  pool.close().await;
  fixture.finalize().await;
  let database = fixture.open().await;

  assert_eq!(
    native_storage_health::latest_records(&database, cancel())
      .await
      .unwrap(),
    Vec::new()
  );
  // Retention over an empty table takes nothing and does not fail.
  assert_eq!(
    native_storage_health::delete_old_data(&database, cancel(), 0)
      .await
      .unwrap(),
    0
  );
  database.close().await.unwrap();
}
