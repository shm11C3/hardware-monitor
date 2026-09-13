#![cfg(feature = "duckdb-archive")]
//! The `AMBIENT_ARCHIVE` and `FAN_ARCHIVE` families, native beside SQLite.
//!
//! Every test here is a differential one: one fixture is seeded through the
//! real SQLite writers, finalized through the real candidate and finalization
//! path, and then the *same question* is put to both engines. A native module
//! is correct here only if it answers what SQLite answers - not if it answers
//! something a reviewer finds reasonable.
//!
//! The fixture is shared across the binary because finalization is the
//! expensive step and none of the read tests mutate it. The one test that does
//! write works on its own copy of the finalized file.

mod native_support;

use chrono::{DateTime, Duration, TimeZone, Utc};
use hardviz_core::infrastructure::database::archive_queries::{
  self, ArchiveBucketTimestamp,
};
use hardviz_core::infrastructure::database::native_database::{
  NativeCancellation, NativeDatabase, NativeDatabaseError, NativeDatabaseOptions,
  ambient_archive as native_ambient, fan_archive as native_fan,
};
use hardviz_core::infrastructure::database::{
  ambient_archive, db, fan_archive, hardware_archive,
};
use hardviz_core::persistence::archive_data::{
  AmbientData, FanArchiveRow, HardwareArchiveRow, HardwareData,
};
use native_support::{NativeFixture, app_native_schema};
use tokio::sync::OnceCell;

/// The window every series test asks about: a couple of hours around the
/// seeded minutes, wide enough that gap filling has gaps to fill.
fn window() -> (DateTime<Utc>, DateTime<Utc>) {
  (at("2026-09-01T00:00:00Z"), at("2026-09-01T00:20:00Z"))
}

fn at(text: &str) -> DateTime<Utc> {
  text.parse().unwrap()
}

fn cancel() -> NativeCancellation {
  NativeCancellation::new()
}

fn reading(avg: f32) -> HardwareData {
  HardwareData {
    avg: Some(avg),
    max: Some(avg + 1.0),
    min: Some(avg - 1.0),
  }
}

fn absent() -> HardwareData {
  HardwareData {
    avg: None,
    max: None,
    min: None,
  }
}

/// The seeded SQLite source and the finalized template copied from it.
///
/// Deliberately does *not* hold an open `NativeDatabase`: see [`Owned`].
struct Shared {
  fixture: NativeFixture,
}

static SHARED: OnceCell<Shared> = OnceCell::const_new();

async fn shared() -> &'static Shared {
  SHARED
    .get_or_init(|| async {
      let fixture = NativeFixture::new();
      // One process-wide pool location for the whole binary, so the SQLite
      // oracle reads the same file the fixture finalizes.
      assert!(db::init(fixture.source.clone()));
      let pool = fixture.migrated_pool().await;
      seed().await;
      pool.close().await;

      fixture.finalize().await;
      Shared { fixture }
    })
    .await
}

/// A private copy of the finalized fixture, plus the owner serving it.
///
/// Every test takes its own copy instead of sharing one open `NativeDatabase`.
/// On Windows a DuckDB file cannot be read, hashed, or opened by a second
/// instance while any owner still holds it, so a shared open handle makes
/// `std::fs::copy` fail with a sharing violation and `read_only` fail with
/// "File is already open". Finalization - the expensive step - stays shared;
/// only the copy is per test.
struct Owned {
  /// Kept alive so [`Owned::path`] stays valid after the database is closed.
  _directory: tempfile::TempDir,
  path: std::path::PathBuf,
  database: Option<NativeDatabase>,
}

impl Owned {
  async fn open(name: &str) -> Self {
    let shared = shared().await;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(name);
    // Safe because nothing holds the finalized template: `shared` deliberately
    // does not keep it open.
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

  /// Release the file so it can be read by a second instance. Call before any
  /// `read_only` on [`Owned::path`].
  async fn close(&mut self) {
    if let Some(database) = self.database.take() {
      database.close().await.unwrap();
    }
  }
}

/// Twenty archive minutes with deliberately awkward shape:
///
/// - two ambient sources, one of which stops reporting half way through, so
///   `sources` and the per-minute averages have to disagree about coverage;
/// - a minute with an ambient reading but no CPU temperature, which must count
///   towards `ambient_avg` and drop out of `delta_avg`;
/// - a fan that reports a real 0 RPM, which is an observation and not a gap;
/// - a sub-millisecond pair straddling SQLite's rounding tie, so the derived
///   epoch keys are exercised rather than assumed.
async fn seed() {
  let base = at("2026-09-01T00:01:00Z");
  for minute in 0..10i64 {
    let tick = base + Duration::minutes(minute);
    let mut ambient = vec![AmbientData {
      source: "Room".to_owned(),
      temperature: 21.0 + minute as f32 * 0.25,
      humidity: Some(48.0),
    }];
    if minute < 5 {
      ambient.push(AmbientData {
        source: "Cabinet".to_owned(),
        // A NULL humidity stays NULL rather than becoming a measured 0%.
        temperature: 24.5 + minute as f32 * 0.1,
        humidity: None,
      });
    }
    ambient_archive::insert(ambient, tick).await.unwrap();

    fan_archive::insert(
      vec![
        FanArchiveRow {
          source: "Exhaust".to_owned(),
          rpm: 800 + minute as u32 * 25,
        },
        FanArchiveRow {
          source: "Intake".to_owned(),
          // A genuine Inactive Fan Reading, stored as the observation it is.
          rpm: if minute == 3 { 0 } else { 1200 },
        },
      ],
      tick,
    )
    .await
    .unwrap();

    hardware_archive::insert(
      HardwareArchiveRow {
        cpu: reading(30.0 + minute as f32),
        memory: reading(40.0),
        // Minute 7 has ambient but no CPU temperature: it counts towards
        // `ambient_avg` and must drop out of `delta_avg`.
        cpu_temperature: if minute == 7 {
          absent()
        } else {
          reading(55.0 + minute as f32 * 0.5)
        },
        cpu_power: reading(12.0),
        gpu_power: absent(),
        ane_power: absent(),
        package_power: absent(),
      },
      tick,
    )
    .await
    .unwrap();
  }

  // The rounding tie, written through the real SQLite writer so finalization
  // derives its key from the bytes sqlx actually stored.
  for tick in TIE_STAMPS {
    ambient_archive::insert(
      vec![AmbientData {
        source: "Room".to_owned(),
        temperature: 30.0,
        humidity: None,
      }],
      at(tick),
    )
    .await
    .unwrap();
    fan_archive::insert(
      vec![FanArchiveRow {
        source: "Exhaust".to_owned(),
        rpm: 999,
      }],
      at(tick),
    )
    .await
    .unwrap();
  }
}

/// `timestamp_millis()` truncates both of these to the same millisecond;
/// SQLite's date parser rounds the second one up.
const TIE_STAMPS: [&str; 2] =
  ["2026-09-01T00:15:00.000499Z", "2026-09-01T00:15:00.000500Z"];

/// The bucket averages compared here go through DuckDB's `AVG(DOUBLE)` on one
/// side and SQLite's compensated `avg()` on the other. They agree bit for bit
/// on archive-magnitude readings, which is what this fixture holds and what a
/// sensor can produce; they are known to diverge once one bucket's finite `f32`
/// values span more than about 2^53 (`[f32::MAX, 1.0, -f32::MAX]`), which is
/// the unresolved binary64 residue the Design Doc records rather than a defect
/// this lane fixes. The fixture deliberately stays inside that boundary, so a
/// failure here means a real disagreement and not the known residue.
#[tokio::test]
async fn ambient_series_answers_what_sqlite_answers() {
  let mut owned = Owned::open("ambient-series.duckdb").await;
  let (start, end) = window();

  for bucket_timestamp in [ArchiveBucketTimestamp::Start, ArchiveBucketTimestamp::End] {
    for width in [60_000i64, 300_000, 600_000] {
      let expected = archive_queries::select_ambient_archive_series(
        &start,
        &end,
        width,
        bucket_timestamp,
      )
      .await
      .unwrap();
      let actual = native_ambient::select_ambient_archive_series(
        owned.database(),
        cancel(),
        &start,
        &end,
        width,
        bucket_timestamp,
      )
      .await
      .unwrap();

      assert_eq!(actual, expected, "width {width}, {bucket_timestamp:?}");
    }
  }

  // The fixture is only evidence if it actually produced the awkward shapes
  // the modules have to get right.
  let series = archive_queries::select_ambient_archive_series(
    &start,
    &end,
    60_000,
    ArchiveBucketTimestamp::Start,
  )
  .await
  .unwrap();
  assert_eq!(
    series.sources,
    vec!["Cabinet".to_owned(), "Room".to_owned()]
  );
  assert!(
    series
      .buckets
      .iter()
      .any(|bucket| bucket.ambient_avg.is_some() && bucket.delta_avg.is_none()),
    "the unpaired minute must count towards ambient_avg and not towards delta_avg"
  );
  assert!(
    series
      .buckets
      .iter()
      .any(|bucket| bucket.ambient_avg.is_none()),
    "the window must contain a gap for the filler to leave empty"
  );
  owned.close().await;
}

#[tokio::test]
async fn fan_series_answers_what_sqlite_answers() {
  let mut owned = Owned::open("fan-series.duckdb").await;
  let (start, end) = window();

  for bucket_timestamp in [ArchiveBucketTimestamp::Start, ArchiveBucketTimestamp::End] {
    for width in [60_000i64, 300_000] {
      let expected =
        archive_queries::select_fan_archive_series(&start, &end, width, bucket_timestamp)
          .await
          .unwrap();
      let actual = native_fan::select_fan_archive_series(
        owned.database(),
        cancel(),
        &start,
        &end,
        width,
        bucket_timestamp,
      )
      .await
      .unwrap();

      assert_eq!(actual, expected, "width {width}, {bucket_timestamp:?}");
    }
  }

  let series = archive_queries::select_fan_archive_series(
    &start,
    &end,
    60_000,
    ArchiveBucketTimestamp::Start,
  )
  .await
  .unwrap();
  assert_eq!(series.len(), 2, "both fans must be present");
  assert!(
    series
      .iter()
      .any(|fan| fan.points.iter().any(|point| point.value == Some(0.0))),
    "a real 0 RPM reading must survive as an observation, not become a gap"
  );
  owned.close().await;
}

#[tokio::test]
async fn fan_rollup_reads_answer_what_sqlite_answers() {
  let mut owned = Owned::open("fan-rollup.duckdb").await;
  let (start, end) = window();

  assert_eq!(
    native_fan::select_fan_minutes_for_range(owned.database(), cancel(), &start, &end)
      .await
      .unwrap(),
    fan_archive::select_fan_minutes_for_range(&start, &end)
      .await
      .unwrap()
  );
  assert_eq!(
    native_fan::has_any_reading(owned.database(), cancel())
      .await
      .unwrap(),
    fan_archive::has_any_reading().await.unwrap()
  );
  for before in [
    at("2026-09-01T00:05:00Z"),
    at("2026-09-01T00:15:00.000500Z"),
    at("2026-09-01T12:00:00Z"),
    // Before everything: both sides must answer `None` rather than an
    // epoch-zero instant.
    at("2020-01-01T00:00:00Z"),
  ] {
    assert_eq!(
      native_fan::max_fan_archive_timestamp_before(owned.database(), cancel(), &before)
        .await
        .unwrap(),
      fan_archive::max_fan_archive_timestamp_before(&before)
        .await
        .unwrap(),
      "before {before}"
    );
  }
  owned.close().await;
}

/// The derived key is what finalization computed from the stored bytes, and
/// finalization runs SQLite's own date parser. The two stamps below differ by
/// one microsecond and land on opposite sides of SQLite's millisecond rounding
/// tie, while `timestamp_millis()` truncates both to the same value - so this
/// is the case a hand-written `timestamp_millis` key would get wrong.
#[tokio::test]
async fn the_derived_key_follows_sqlites_rounding_not_rusts_truncation() {
  // Its own copy, released before a second DuckDB instance reads it: on
  // Windows the file cannot be opened twice while an owner holds it.
  let mut owned = Owned::open("derived-key.duckdb").await;
  owned.close().await;
  let connection = native_support::read_only(&owned.path);
  let mut statement = connection
    .prepare(
      "SELECT timestamp, __hv_timestamp_epoch_ms
       FROM AMBIENT_ARCHIVE
       WHERE timestamp LIKE '2026-09-01T00:15:00.0005%'
          OR timestamp LIKE '2026-09-01T00:15:00.0004%'
       ORDER BY timestamp ASC",
    )
    .unwrap();
  let rows: Vec<(String, Option<i64>)> = statement
    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap();

  assert_eq!(
    rows,
    vec![
      (
        "2026-09-01T00:15:00.000499+00:00".to_owned(),
        Some(1_788_221_700_000)
      ),
      (
        "2026-09-01T00:15:00.000500+00:00".to_owned(),
        Some(1_788_221_700_001)
      ),
    ],
    "SQLite rounds the .000500 stamp up; Rust's timestamp_millis truncates both"
  );
  // The truncating key both stamps would have received.
  assert_eq!(
    at(TIE_STAMPS[0]).timestamp_millis(),
    at(TIE_STAMPS[1]).timestamp_millis()
  );
  assert_eq!(at(TIE_STAMPS[0]).timestamp_millis(), 1_788_221_700_000);
}

/// A native write has to produce the same two things the SQLite writer
/// produces: the same stored bytes, and the key finalization would have
/// derived from them. Written into a copy of the finalized file so the read
/// tests keep seeing the fixture they were seeded with.
#[tokio::test]
async fn a_native_write_stores_the_bytes_and_key_sqlite_would() {
  let mut owned = Owned::open("writable.duckdb").await;

  // The same two tie stamps, now written by the native writer rather than
  // recovered from a finalized SQLite row.
  for tick in TIE_STAMPS {
    native_ambient::insert(
      owned.database(),
      cancel(),
      vec![AmbientData {
        source: "Native".to_owned(),
        temperature: 30.0,
        humidity: None,
      }],
      at(tick),
    )
    .await
    .unwrap();
    native_fan::insert(
      owned.database(),
      cancel(),
      vec![FanArchiveRow {
        source: "Native".to_owned(),
        rpm: 999,
      }],
      at(tick),
    )
    .await
    .unwrap();
  }
  owned.close().await;

  let connection = native_support::read_only(&owned.path);
  for (table, label) in [("AMBIENT_ARCHIVE", "Native"), ("FAN_ARCHIVE", "Native")] {
    let mut statement = connection
      .prepare(&format!(
        "SELECT timestamp, __hv_timestamp_epoch_ms FROM {table}
         WHERE source = '{label}' ORDER BY timestamp ASC"
      ))
      .unwrap();
    let written: Vec<(String, Option<i64>)> = statement
      .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
      .unwrap()
      .collect::<Result<_, _>>()
      .unwrap();
    assert_eq!(
      written,
      vec![
        (
          "2026-09-01T00:15:00.000499+00:00".to_owned(),
          Some(1_788_221_700_000)
        ),
        (
          "2026-09-01T00:15:00.000500+00:00".to_owned(),
          Some(1_788_221_700_001)
        ),
      ],
      "{table}"
    );
  }

  // And the bytes match what SQLite's own writer stored for the same instants
  // (the rows seeded through `ambient_archive::insert`).
  let seeded: Vec<String> = {
    let mut statement = connection
      .prepare(
        "SELECT DISTINCT timestamp FROM AMBIENT_ARCHIVE
         WHERE source = 'Room' AND timestamp LIKE '2026-09-01T00:15:%'
         ORDER BY timestamp ASC",
      )
      .unwrap();
    statement
      .query_map([], |row| row.get(0))
      .unwrap()
      .collect::<Result<_, _>>()
      .unwrap()
  };
  assert_eq!(
    seeded,
    vec![
      "2026-09-01T00:15:00.000499+00:00".to_owned(),
      "2026-09-01T00:15:00.000500+00:00".to_owned(),
    ]
  );
}

/// Retention is a byte-wise text comparison in both engines, so the same
/// cutoff has to take the same rows.
#[tokio::test]
async fn native_retention_takes_the_same_rows_as_sqlite() {
  let mut owned = Owned::open("retained.duckdb").await;
  let native = owned.database();

  // Every seeded row is far in the past relative to `Utc::now()`, so a
  // zero-day retention must take all of them and report the count.
  let ambient_before = count(native, "AMBIENT_ARCHIVE").await;
  let fan_before = count(native, "FAN_ARCHIVE").await;
  assert!(ambient_before > 0 && fan_before > 0);

  assert_eq!(
    native_ambient::delete_old_data(native, cancel(), 0)
      .await
      .unwrap(),
    ambient_before
  );
  assert_eq!(
    native_fan::delete_old_data(native, cancel(), 0)
      .await
      .unwrap(),
    fan_before
  );
  assert_eq!(count(native, "AMBIENT_ARCHIVE").await, 0);
  assert_eq!(count(native, "FAN_ARCHIVE").await, 0);

  // A second pass deletes nothing rather than failing.
  assert_eq!(
    native_ambient::delete_old_data(native, cancel(), 0)
      .await
      .unwrap(),
    0
  );
  owned.close().await;
}

async fn count(native: &NativeDatabase, table: &'static str) -> u64 {
  let sql = format!("SELECT COUNT(*) FROM {table}");
  native
    .request_read(cancel(), move |context| {
      Ok(
        context
          .connection()
          .query_row(&sql, [], |row| row.get::<_, i64>(0))
          .unwrap() as u64,
      )
    })
    .await
    .unwrap()
}

/// The stored-text decoder is a transcription of sqlx's own grammar, so a
/// stamp written by sqlx must read back through the native path as the same
/// instant sqlx decodes it to. Exercised through the public native reader
/// rather than against the private module, so it pins the behaviour callers
/// actually get.
#[tokio::test]
async fn native_decoding_agrees_with_sqlx_on_every_spelling_the_archive_carries() {
  let mut owned = Owned::open("decoding.duckdb").await;
  let (start, end) = window();

  let native =
    native_fan::select_fan_minutes_for_range(owned.database(), cancel(), &start, &end)
      .await
      .unwrap();
  let sqlite = fan_archive::select_fan_minutes_for_range(&start, &end)
    .await
    .unwrap();
  assert_eq!(native, sqlite);
  assert!(
    !native.is_empty(),
    "the comparison is only evidence if rows came back"
  );
  // Including the sub-millisecond stamps, whose fractional part is exactly
  // where a looser parser would disagree with sqlx.
  assert!(
    native.iter().any(
      |sample| sample.timestamp == Utc.timestamp_micros(1_788_221_700_000_500).unwrap()
    ),
    "the .000500 stamp must decode to the microsecond, not to the rounded key"
  );
  owned.close().await;
}

/// The one place both engines can be asked the NaN question directly: the
/// ambient writer is public on both sides, and `humidity` is nullable while
/// `temperature` is `NOT NULL`, so one table covers both of SQLite's NaN
/// behaviours.
///
/// The probes are stamped far outside every window the other tests read, so
/// the SQLite side can be written after the fixture was finalized without
/// disturbing a single comparison above.
const NAN_PROBE_STAMP: &str = "2030-01-01T00:00:00Z";

#[tokio::test]
async fn a_nan_reading_is_the_same_absence_in_both_engines() {
  let shared = shared().await;

  // SQLite, through the production writer: a NaN humidity is stored as NULL,
  // and a finite one beside it is stored as a real.
  ambient_archive::insert(
    vec![
      AmbientData {
        source: "NaN probe".to_owned(),
        temperature: 21.0,
        humidity: Some(f32::NAN),
      },
      AmbientData {
        source: "Finite probe".to_owned(),
        temperature: 21.0,
        humidity: Some(48.0),
      },
    ],
    at(NAN_PROBE_STAMP),
  )
  .await
  .unwrap();

  let pool = native_support::open_pool(&shared.fixture.source, false).await;
  let stored: Vec<(String, String)> = sqlx::query_as(
    "SELECT source, typeof(humidity) FROM AMBIENT_ARCHIVE
     WHERE source IN ('NaN probe', 'Finite probe') ORDER BY source ASC",
  )
  .fetch_all(&pool)
  .await
  .unwrap();
  pool.close().await;
  assert_eq!(
    stored,
    vec![
      ("Finite probe".to_owned(), "real".to_owned()),
      ("NaN probe".to_owned(), "null".to_owned()),
    ],
    "SQLite stores a bound NaN as NULL and leaves a finite reading alone"
  );

  // The native writer, given the same two rows, has to reach the same state.
  let mut owned = Owned::open("nan.duckdb").await;
  native_ambient::insert(
    owned.database(),
    cancel(),
    vec![
      AmbientData {
        source: "NaN probe".to_owned(),
        temperature: 21.0,
        humidity: Some(f32::NAN),
      },
      AmbientData {
        source: "Finite probe".to_owned(),
        temperature: 21.0,
        humidity: Some(48.0),
      },
    ],
    at(NAN_PROBE_STAMP),
  )
  .await
  .unwrap();
  owned.close().await;

  let connection = native_support::read_only(&owned.path);
  let mut statement = connection
    .prepare(
      "SELECT source, CASE WHEN humidity IS NULL THEN 'null'
                           WHEN isnan(humidity) THEN 'nan'
                           ELSE 'real' END
       FROM AMBIENT_ARCHIVE
       WHERE source IN ('NaN probe', 'Finite probe') ORDER BY source ASC",
    )
    .unwrap();
  let written: Vec<(String, String)> = statement
    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap();
  assert_eq!(written, stored, "native nullness must match SQLite's");
}

/// `AMBIENT_ARCHIVE.temperature` and `PROCESS_STATS.cpu_usage` are `NOT NULL`,
/// so SQLite does not absorb the NaN into a gap - it refuses the row. Both
/// writers must refuse it, and both must leave nothing behind.
#[tokio::test]
async fn a_nan_in_a_required_column_is_refused_by_both_engines() {
  let shared = shared().await;
  let refused = at("2030-06-01T00:00:00Z");

  let sqlite_error = ambient_archive::insert(
    vec![AmbientData {
      source: "Refused probe".to_owned(),
      temperature: f32::NAN,
      humidity: None,
    }],
    refused,
  )
  .await
  .unwrap_err();
  assert!(
    matches!(&sqlite_error, sqlx::Error::Database(error)
      if error.message().contains("NOT NULL constraint failed")),
    "{sqlite_error:?}"
  );

  let pool = native_support::open_pool(&shared.fixture.source, false).await;
  let surviving: i64 = sqlx::query_scalar(
    "SELECT COUNT(*) FROM AMBIENT_ARCHIVE WHERE source = 'Refused probe'",
  )
  .fetch_one(&pool)
  .await
  .unwrap();
  pool.close().await;
  assert_eq!(surviving, 0, "SQLite wrote nothing");

  let mut owned = Owned::open("refused.duckdb").await;
  let native_error = native_ambient::insert(
    owned.database(),
    cancel(),
    vec![AmbientData {
      source: "Refused probe".to_owned(),
      temperature: f32::NAN,
      humidity: None,
    }],
    refused,
  )
  .await
  .unwrap_err();
  assert!(
    matches!(
      native_error,
      NativeDatabaseError::NotANumberInRequiredColumn {
        table: "AMBIENT_ARCHIVE",
        column: "temperature"
      }
    ),
    "{native_error:?}"
  );
  assert_eq!(
    count_where(
      owned.database(),
      "AMBIENT_ARCHIVE",
      "source = 'Refused probe'"
    )
    .await,
    0
  );
  owned.close().await;
}

async fn count_where(
  native: &NativeDatabase,
  table: &'static str,
  predicate: &'static str,
) -> u64 {
  let sql = format!("SELECT COUNT(*) FROM {table} WHERE {predicate}");
  native
    .request_read(cancel(), move |context| {
      Ok(
        context
          .connection()
          .query_row(&sql, [], |row| row.get::<_, i64>(0))
          .unwrap() as u64,
      )
    })
    .await
    .unwrap()
}
