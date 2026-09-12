#![cfg(feature = "duckdb-archive")]
//! Whether the native GPU_DATA_ARCHIVE family answers what the SQLite family
//! answers.
//!
//! The same method as `duckdb_data_archive`: one fixture through the production
//! SQLite writer and query, and through candidate -> finalize -> native, with
//! the answers compared bit for bit. What this family adds is the *subject*.
//! GPU series are keyed by the archived `gpu_name` (ADR 0019), so the fixture
//! contains two adapters that once reported the same name, a name holding an
//! embedded NUL, and the `'Unknown'` placeholder - the cases where a
//! reimplementation would be tempted to reconstruct an inventory id, normalize
//! bytes, or treat a placeholder as a missing row.
//!
//! It also carries the storage classes this table mixes for a different reason
//! than DATA_ARCHIVE does: `usage_*` and `temperature_avg` are written from
//! `Option<f32>` into INTEGER-declared columns, while the remaining counts come
//! from `Option<i32>` and are integers in every row.

mod native_support;

use chrono::{DateTime, Duration, Timelike, Utc};
use hardviz_core::infrastructure::database::archive_queries::{
  ArchiveBucketTimestamp, ArchiveSeriesError, ArchiveSeriesPoint, GpuArchiveColumn,
};
use hardviz_core::infrastructure::database::native_database::{
  NativeCancellation, NativeDatabase, NativeDatabaseError, NativeSeriesWindow,
  gpu_archive as native_gpu,
};
use hardviz_core::infrastructure::database::{archive_queries, db, gpu_archive, migrate};
use hardviz_core::persistence::archive_data::GpuData;
use native_support::{
  NativeFixture, app_migrations, open_pool, read_only, sqlite_epoch_milliseconds_of,
};
use sqlx::{Executor, Row};

/// The adapter every series below is keyed by, and the second adapter that
/// reported the same name.
const SHARED_NAME: &str = "Apple M4 Max";
/// A name whose bytes a reimplementation could easily truncate.
const NUL_NAME: &str = "Discrete GPU\0Secondary";

const COLUMNS: [(GpuArchiveColumn, &str); 9] = [
  (GpuArchiveColumn::UsageAvg, "usage_avg"),
  (GpuArchiveColumn::UsageMax, "usage_max"),
  (GpuArchiveColumn::UsageMin, "usage_min"),
  (GpuArchiveColumn::TemperatureAvg, "temperature_avg"),
  (GpuArchiveColumn::TemperatureMax, "temperature_max"),
  (GpuArchiveColumn::TemperatureMin, "temperature_min"),
  (GpuArchiveColumn::DedicatedMemoryAvg, "dedicated_memory_avg"),
  (GpuArchiveColumn::DedicatedMemoryMax, "dedicated_memory_max"),
  (GpuArchiveColumn::DedicatedMemoryMin, "dedicated_memory_min"),
];

/// The columns a finalized archive holds as `UNION(i BIGINT, r DOUBLE)`: the
/// INTEGER-declared ones whose writer binds `Option<f32>`.
const TAGGED_COLUMNS: [&str; 4] =
  ["usage_avg", "usage_max", "usage_min", "temperature_avg"];

#[tokio::test]
async fn the_native_gpu_archive_family_reproduces_the_sqlite_family() {
  let fixture = NativeFixture::new();
  // The only test in this binary that initializes Core's process-wide database
  // path, so the SQLite side runs through the real writer and the real query.
  assert!(db::init(fixture.source.clone()));
  migrate::run(app_migrations::get_migrations())
    .await
    .unwrap();

  seed_through_the_production_writer().await;
  seed_legacy_rows().await;

  // The conversion is the one moment the whole archive is read, so it is where
  // the unreadable stamp is counted. The count is informational - the row is
  // copied intact and every query that is not bucketed by time still sees it.
  let report = fixture.finalize().await;
  for table in &report.tables {
    let expected = u64::from(table.name == "GPU_DATA_ARCHIVE");
    assert_eq!(table.unconvertible_timestamps, expected, "{}", table.name);
  }
  let database = fixture.open().await;

  compare_every_series(&database).await;
  compare_the_subject_list(&database).await;
  compare_the_refusals(&database).await;
  compare_an_unreadable_timestamp(&database).await;

  let write_instant = (Utc::now() - Duration::days(1))
    .with_nanosecond(500_000)
    .unwrap();
  let written = parity_row();
  gpu_archive::insert(written.clone(), write_instant)
    .await
    .unwrap();
  native_gpu::insert(&database, NativeCancellation::new(), written, write_instant)
    .await
    .unwrap();

  // A NaN reading beside an ordinary one, through both writers.
  let nan_minute = write_the_nan_minute(&database).await;

  let before = sqlite_row_count(&fixture).await;
  gpu_archive::delete_old_data(7).await.unwrap();
  let expected_deleted = before - sqlite_row_count(&fixture).await;
  assert!(
    expected_deleted >= 1,
    "the Retention Period must bisect the fixture"
  );
  let deleted = native_gpu::delete_old_data(&database, NativeCancellation::new(), 7)
    .await
    .unwrap();
  assert_eq!(i64::try_from(deleted).unwrap(), expected_deleted);

  compare_the_nan_minute(&database, nan_minute).await;

  compare_the_surviving_rows(&fixture, database).await;
}

/// Rows written by the production writer.
///
/// Two of them carry the same `gpu_name` under different `gpu_id`s: ADR 0019
/// says the archived name is the subject a user picked, so those two minutes
/// are one subject's history and must aggregate together in both engines.
async fn seed_through_the_production_writer() {
  let rows: [(&str, Option<&str>, &str, f32); 5] = [
    (
      "2026-09-01T00:00:00Z",
      Some("gpu-persisted-name"),
      SHARED_NAME,
      10.0,
    ),
    (
      "2026-09-01T00:00:30.125Z",
      Some("gpu-other-adapter"),
      SHARED_NAME,
      20.0,
    ),
    ("2026-09-01T00:00:30.125Z", None, SHARED_NAME, 30.0),
    (
      "2026-09-01T00:02:00.000500Z",
      Some("gpu-nul"),
      NUL_NAME,
      40.0,
    ),
    ("2026-09-01T00:03:00Z", Some("gpu-unknown"), "Unknown", 50.0),
  ];
  for (text, gpu_id, gpu_name, seed) in rows {
    let instant: DateTime<Utc> = text.parse().unwrap();
    gpu_archive::insert(
      GpuData {
        gpu_id: gpu_id.map(str::to_owned),
        gpu_name: gpu_name.to_owned(),
        usage_avg: Some(seed),
        usage_max: Some(seed + 0.5),
        usage_min: Some(seed - 0.5),
        temperature_avg: Some(40.0 + seed / 10.0),
        temperature_max: Some(60),
        temperature_min: Some(30),
        dedicated_memory_avg: Some(1024),
        dedicated_memory_max: Some(2048),
        dedicated_memory_min: Some(512),
      },
      instant,
    )
    .await
    .unwrap();
  }

  // One row placed relative to the clock rather than on a fixed date, so the
  // Retention Period compared below bisects the fixture whenever the suite
  // runs. Five years back keeps it clear of every range compared above.
  gpu_archive::insert(
    GpuData {
      gpu_id: Some("gpu-expired".to_owned()),
      gpu_name: SHARED_NAME.to_owned(),
      usage_avg: Some(1.5),
      usage_max: Some(2.5),
      usage_min: Some(0.5),
      temperature_avg: Some(41.5),
      temperature_max: Some(61),
      temperature_min: Some(31),
      dedicated_memory_avg: Some(1025),
      dedicated_memory_max: Some(2049),
      dedicated_memory_min: Some(513),
    },
    Utc::now() - Duration::days(1_826),
  )
  .await
  .unwrap();
}

/// Rows a released build could have left behind: integer storage classes in the
/// INTEGER-declared columns, an all-NULL minute, spellings that sort outside
/// the ISO-8601 UTC block, and one stamp SQLite cannot read at all.
async fn seed_legacy_rows() {
  let pool = db::get_pool().await.unwrap();
  pool
    .execute(
      r#"
      INSERT INTO GPU_DATA_ARCHIVE
        (gpu_id,gpu_name,usage_avg,usage_max,usage_min,temperature_avg,
         temperature_max,temperature_min,dedicated_memory_avg,
         dedicated_memory_max,dedicated_memory_min,timestamp)
      VALUES
        ('legacy-a','Apple M4 Max',3,9007199254740993,-9007199254740993,41,
         70,20,4096,9223372036854775807,-9223372036854775808,
         '2026-09-01T00:00:10Z'),
        ('legacy-b','Apple M4 Max',0.5,0.5,0.5,-0.0,
         71,21,4097,8192,256,'2026-09-01T00:00:20Z'),
        ('legacy-c','Apple M4 Max',NULL,NULL,NULL,NULL,
         NULL,NULL,NULL,NULL,NULL,'2026-09-01T00:00:40Z'),
        ('legacy-d','Apple M4 Max',7,3.5,2,12,
         72,22,4098,8193,257,'2026-09-01 00:01:10'),
        ('legacy-e','Apple M4 Max',1,2,3,13,
         73,23,4099,8194,258,'2026-09-01T09:01:20+09:00'),
        ('legacy-f','Apple M4 Max',2,3,4,14,
         74,24,4100,8195,259,'2026-09-01T00:01:30.500Z'),
        ('legacy-g','Apple M4 Max',8,9,10,15,
         75,25,4101,8196,260,'2099-13-45T99:99:99+00:00');
      "#,
    )
    .await
    .unwrap();
}

/// Every column, for the shared subject and for the NUL-bearing one, over the
/// same endpoint cases the DATA_ARCHIVE family is compared on.
async fn compare_every_series(database: &NativeDatabase) {
  let ranges: [(&str, &str); 5] = [
    ("2026-08-31T23:59:00Z", "2026-09-01T00:10:00Z"),
    ("2026-09-01T00:00:00Z", "2026-09-01T00:05:00Z"),
    ("2026-09-01T00:00:30.125Z", "2026-09-01T00:01:30.500Z"),
    ("2026-09-01T00:00:00Z", "2026-09-01T00:00:00Z"),
    ("2027-01-01T00:00:00Z", "2027-01-01T00:01:00Z"),
  ];
  // The minute holding the same subject under two adapter ids, plus the legacy
  // integer cells.
  const SHARED_MINUTE: i64 = 1_788_220_800_000;

  let mut compared = 0_usize;
  let mut shared_usage_max = None;
  for name in [SHARED_NAME, NUL_NAME, "Unknown", "no such adapter"] {
    for (column, column_name) in COLUMNS {
      for (start, end) in ranges {
        let start: DateTime<Utc> = start.parse().unwrap();
        let end: DateTime<Utc> = end.parse().unwrap();
        for width in [1_000_i64, 60_000, 300_000] {
          for bucket in [ArchiveBucketTimestamp::Start, ArchiveBucketTimestamp::End] {
            let expected = archive_queries::select_gpu_archive_series(
              column, name, &start, &end, width, bucket,
            )
            .await
            .unwrap();
            let actual = native_gpu::select_gpu_archive_series(
              database,
              NativeCancellation::new(),
              column,
              name,
              window(&start, &end, width, bucket),
            )
            .await
            .unwrap();
            assert_eq!(
              bits(&actual),
              bits(&expected),
              "{column_name} for {name:?} over {start}..{end} at {width}ms, \
               bucket {bucket:?}"
            );
            compared += 1;
            let recorded =
              (name == SHARED_NAME && column_name == "usage_max" && width == 60_000)
                .then(|| {
                  expected
                    .iter()
                    .find(|point| point.timestamp == SHARED_MINUTE)
                    .and_then(|point| point.value)
                })
                .flatten();
            shared_usage_max = recorded.or(shared_usage_max);
          }
        }
      }
    }
  }
  assert_eq!(compared, 4 * COLUMNS.len() * ranges.len() * 3 * 2);

  // One subject, two adapter ids, one answer - and the legacy integer past 2^53
  // rounded through the cast rather than compared exactly, the same as it is in
  // DATA_ARCHIVE.
  assert_eq!(
    shared_usage_max.map(f64::to_bits),
    Some(9_007_199_254_740_992.0_f64.to_bits()),
    "the shared minute must aggregate both adapters through the cast"
  );
}

/// The subject list: the same names, in the same order, with `'Unknown'`
/// excluded as a stored value that names no adapter and the NUL-bearing name
/// kept whole.
async fn compare_the_subject_list(database: &NativeDatabase) {
  let expected = archive_queries::select_gpu_names().await.unwrap();
  let actual = native_gpu::select_gpu_names(database, NativeCancellation::new())
    .await
    .unwrap();
  assert_eq!(actual, expected);
  assert_eq!(
    expected,
    vec![SHARED_NAME.to_owned(), NUL_NAME.to_owned()],
    "the placeholder is excluded and the embedded NUL survives"
  );
}

async fn compare_the_refusals(database: &NativeDatabase) {
  let start: DateTime<Utc> = "2026-09-01T00:00:00Z".parse().unwrap();
  let end: DateTime<Utc> = "2026-09-02T00:00:00Z".parse().unwrap();

  assert!(matches!(
    refusal(database, &end, &start, 60_000).await,
    (
      ArchiveSeriesError::InvalidTimeRange,
      ArchiveSeriesError::InvalidTimeRange
    )
  ));
  assert!(matches!(
    refusal(database, &start, &end, 0).await,
    (
      ArchiveSeriesError::InvalidBucketWidth,
      ArchiveSeriesError::InvalidBucketWidth
    )
  ));
  match refusal(database, &start, &end, 1_000).await {
    (
      ArchiveSeriesError::TooManyPoints {
        requested: expected_requested,
        maximum: expected_maximum,
      },
      ArchiveSeriesError::TooManyPoints { requested, maximum },
    ) => {
      assert_eq!(requested, expected_requested);
      assert_eq!(maximum, expected_maximum);
      assert!(requested > maximum);
    }
    other => panic!("unexpected refusal pair: {other:?}"),
  }
}

async fn refusal(
  database: &NativeDatabase,
  start: &DateTime<Utc>,
  end: &DateTime<Utc>,
  width: i64,
) -> (ArchiveSeriesError, ArchiveSeriesError) {
  let expected = archive_queries::select_gpu_archive_series(
    GpuArchiveColumn::UsageAvg,
    SHARED_NAME,
    start,
    end,
    width,
    ArchiveBucketTimestamp::Start,
  )
  .await
  .unwrap_err();
  let actual = native_gpu::select_gpu_archive_series(
    database,
    NativeCancellation::new(),
    GpuArchiveColumn::UsageAvg,
    SHARED_NAME,
    window(start, end, width, ArchiveBucketTimestamp::Start),
  )
  .await
  .unwrap_err();
  (expected, same_refusal(actual))
}

/// The native families return their own error type, so a refusal arrives
/// wrapped. Unwrapping it here is itself a claim: a request the SQLite path
/// refuses must be refused natively for the same reason, not merely refused.
fn same_refusal(error: NativeDatabaseError) -> ArchiveSeriesError {
  match error {
    NativeDatabaseError::ArchiveSeries { source } => source,
    other => panic!("expected a range refusal, got {other:?}"),
  }
}

/// The same parity for this table - see
/// `duckdb_data_archive::compare_an_unreadable_timestamp` for the mechanism
/// each engine arrives at it by.
async fn compare_an_unreadable_timestamp(database: &NativeDatabase) {
  let start: DateTime<Utc> = "2099-01-01T00:00:00Z".parse().unwrap();
  let end: DateTime<Utc> = "2100-01-01T00:00:00Z".parse().unwrap();

  for bucket in [ArchiveBucketTimestamp::Start, ArchiveBucketTimestamp::End] {
    let expected = archive_queries::select_gpu_archive_series(
      GpuArchiveColumn::UsageAvg,
      SHARED_NAME,
      &start,
      &end,
      86_400_000,
      bucket,
    )
    .await
    .unwrap();
    let actual = native_gpu::select_gpu_archive_series(
      database,
      NativeCancellation::new(),
      GpuArchiveColumn::UsageAvg,
      SHARED_NAME,
      window(&start, &end, 86_400_000, bucket),
    )
    .await
    .unwrap();
    assert_eq!(bits(&actual), bits(&expected), "bucket {bucket:?}");
    assert!(
      expected.iter().all(|point| point.value.is_none()),
      "the row SQLite cannot read has no bucket, so the range is all gaps"
    );
  }
}

async fn compare_the_surviving_rows(fixture: &NativeFixture, database: NativeDatabase) {
  // Reading the finalized file directly means taking it away from its owner
  // first: on Windows a second DuckDB handle - a read-only one included - is
  // refused while the owner still holds the file, and the same is true of
  // hashing or copying it. Taking the owner by value makes that ordering
  // structural rather than a rule a later edit could quietly break.
  database.close().await.unwrap();
  drop(database);

  let pool = open_pool(&fixture.source, false).await;
  let connection = read_only(&fixture.finalized);

  let expected_ids: Vec<i64> = sqlx::query("SELECT id FROM GPU_DATA_ARCHIVE ORDER BY id")
    .fetch_all(&pool)
    .await
    .unwrap()
    .into_iter()
    .map(|row| row.get(0))
    .collect();
  let actual_ids: Vec<i64> = connection
    .prepare("SELECT id FROM GPU_DATA_ARCHIVE ORDER BY id")
    .unwrap()
    .query_map([], |row| row.get(0))
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap();
  assert_eq!(actual_ids, expected_ids);
  // The last three rows are the ones both writers wrote: the parity row, then
  // the NaN minute's ordinary row and its NaN row.
  let inspected = &expected_ids[expected_ids.len() - 3..];
  let written_id = inspected[0];

  let expected_stamp: (String, i64) = sqlx::query(&format!(
    "SELECT CAST(timestamp AS TEXT), {} FROM GPU_DATA_ARCHIVE WHERE id = ?",
    sqlite_epoch_milliseconds_of("timestamp")
  ))
  .bind(written_id)
  .fetch_one(&pool)
  .await
  .map(|row| (row.get(0), row.get(1)))
  .unwrap();
  let actual_stamp: (String, i64) = connection
    .query_row(
      "SELECT \"timestamp\", __hv_timestamp_epoch_ms \
       FROM GPU_DATA_ARCHIVE WHERE id = ?",
      [written_id],
      |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .unwrap();
  assert_eq!(actual_stamp, expected_stamp);

  // The subject keys, as stored bytes rather than as a normalized name.
  let expected_subject: (Option<String>, String) =
    sqlx::query("SELECT gpu_id, gpu_name FROM GPU_DATA_ARCHIVE WHERE id = ?")
      .bind(written_id)
      .fetch_one(&pool)
      .await
      .map(|row| (row.get(0), row.get(1)))
      .unwrap();
  let actual_subject: (Option<String>, String) = connection
    .query_row(
      "SELECT gpu_id, gpu_name FROM GPU_DATA_ARCHIVE WHERE id = ?",
      [written_id],
      |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .unwrap();
  assert_eq!(actual_subject, expected_subject);
  assert_eq!(actual_subject.1.as_bytes(), NUL_NAME.as_bytes());

  for (&written_id, (_, name)) in inspected
    .iter()
    .flat_map(|id| std::iter::repeat(id).zip(COLUMNS))
  {
    let expected: (String, Option<i64>, Option<f64>) = sqlx::query(&format!(
      "SELECT typeof({name}), \
       CASE WHEN typeof({name}) = 'integer' THEN {name} END, \
       CASE WHEN typeof({name}) = 'real' THEN {name} END \
       FROM GPU_DATA_ARCHIVE WHERE id = ?"
    ))
    .bind(written_id)
    .fetch_one(&pool)
    .await
    .map(|row| (row.get(0), row.get(1), row.get(2)))
    .unwrap();
    let actual: (Option<String>, Option<i64>, Option<f64>) =
      if TAGGED_COLUMNS.contains(&name) {
        connection
          .query_row(
            &format!(
              "SELECT CAST(union_tag({name}) AS VARCHAR), \
               union_extract({name}, 'i'), union_extract({name}, 'r') \
               FROM GPU_DATA_ARCHIVE WHERE id = ?"
            ),
            [written_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
          )
          .unwrap()
      } else {
        connection
          .query_row(
            &format!(
              "SELECT CASE WHEN {name} IS NULL THEN NULL ELSE 'i' END, \
               {name}, NULL::DOUBLE FROM GPU_DATA_ARCHIVE WHERE id = ?"
            ),
            [written_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
          )
          .unwrap()
      };
    let expected_tag = match expected.0.as_str() {
      "null" => None,
      "integer" => Some("i".to_owned()),
      "real" => Some("r".to_owned()),
      other => panic!("{name} was written as {other}, which no writer produces"),
    };
    assert_eq!(actual.0, expected_tag, "{name} at id {written_id}");
    assert_eq!(actual.1, expected.1, "{name} at id {written_id}");
    assert_eq!(
      actual.2.map(f64::to_bits),
      expected.2.map(f64::to_bits),
      "{name} at id {written_id}"
    );
    // No cell on either side is a stored NaN: SQLite cannot hold one.
    assert!(!expected.2.is_some_and(f64::is_nan), "{name}");
    assert!(!actual.2.is_some_and(f64::is_nan), "{name}");
  }
  pool.close().await;
  // Before the fixture's temporary directory goes, so the file is not removed
  // out from under an open handle.
  drop(connection);
}

/// One minute written through both writers: an ordinary row, then a row whose
/// `Option<f32>` readings are NaN.
///
/// Only those four columns can carry a NaN at all - the rest of this table is
/// written from `Option<i32>` - so they are the whole surface the bind rule has
/// to cover here. See `duckdb_data_archive::write_the_nan_minute` for why the
/// ordinary row beside it is what makes the comparison say anything.
async fn write_the_nan_minute(database: &NativeDatabase) -> DateTime<Utc> {
  let minute = (Utc::now() - Duration::days(2))
    .with_second(0)
    .unwrap()
    .with_nanosecond(0)
    .unwrap();
  for (offset, readings) in [
    (
      Duration::seconds(30),
      [Some(4.0), Some(6.0), Some(2.0), Some(44.0)],
    ),
    (
      Duration::seconds(31),
      [
        Some(f32::NAN),
        Some(f32::NAN),
        Some(-f32::NAN),
        Some(f32::NAN),
      ],
    ),
  ] {
    let row = GpuData {
      gpu_id: Some("gpu-nan".to_owned()),
      gpu_name: SHARED_NAME.to_owned(),
      usage_avg: readings[0],
      usage_max: readings[1],
      usage_min: readings[2],
      temperature_avg: readings[3],
      temperature_max: Some(65),
      temperature_min: Some(35),
      dedicated_memory_avg: Some(2048),
      dedicated_memory_max: Some(4096),
      dedicated_memory_min: Some(1024),
    };
    let instant = minute + offset;
    gpu_archive::insert(row.clone(), instant).await.unwrap();
    native_gpu::insert(database, NativeCancellation::new(), row, instant)
      .await
      .unwrap();
  }
  minute
}

/// What each engine answers for the minute holding the NaN row.
async fn compare_the_nan_minute(database: &NativeDatabase, minute: DateTime<Utc>) {
  let start = minute;
  let end = minute + Duration::seconds(59);
  for (column, name) in [
    (GpuArchiveColumn::UsageAvg, "usage_avg"),
    (GpuArchiveColumn::UsageMax, "usage_max"),
    (GpuArchiveColumn::UsageMin, "usage_min"),
    (GpuArchiveColumn::TemperatureAvg, "temperature_avg"),
  ] {
    for bucket in [ArchiveBucketTimestamp::Start, ArchiveBucketTimestamp::End] {
      let expected = archive_queries::select_gpu_archive_series(
        column,
        SHARED_NAME,
        &start,
        &end,
        60_000,
        bucket,
      )
      .await
      .unwrap();
      let actual = native_gpu::select_gpu_archive_series(
        database,
        NativeCancellation::new(),
        column,
        SHARED_NAME,
        window(&start, &end, 60_000, bucket),
      )
      .await
      .unwrap();
      assert_eq!(bits(&actual), bits(&expected), "{name}, bucket {bucket:?}");
      let answered = expected.iter().filter_map(|point| point.value).next();
      assert_eq!(
        answered.map(f64::is_nan),
        Some(false),
        "{name} must answer the ordinary reading, not a NaN or a gap"
      );
    }
  }
}

/// A row carrying the values a GPU writer can produce that a test would not
/// think to spell: an integral reading into an INTEGER-declared column, a
/// fractional one beside it, absent readings, the extremes of `i32`, and a name
/// with an embedded NUL.
fn parity_row() -> GpuData {
  GpuData {
    gpu_id: Some("gpu-parity".to_owned()),
    gpu_name: NUL_NAME.to_owned(),
    usage_avg: Some(-0.0),
    usage_max: Some(99.5),
    usage_min: None,
    temperature_avg: Some(42.0),
    temperature_max: Some(i32::MAX),
    temperature_min: Some(i32::MIN),
    dedicated_memory_avg: None,
    dedicated_memory_max: Some(0),
    dedicated_memory_min: Some(-1),
  }
}

fn window<'a>(
  start: &'a DateTime<Utc>,
  end: &'a DateTime<Utc>,
  bucket_width_ms: i64,
  bucket_timestamp: ArchiveBucketTimestamp,
) -> NativeSeriesWindow<'a> {
  NativeSeriesWindow {
    start,
    end,
    bucket_width_ms,
    bucket_timestamp,
  }
}

fn bits(series: &[ArchiveSeriesPoint]) -> Vec<(i64, Option<u64>)> {
  series
    .iter()
    .map(|point| (point.timestamp, point.value.map(f64::to_bits)))
    .collect()
}

async fn sqlite_row_count(fixture: &NativeFixture) -> i64 {
  let pool = open_pool(&fixture.source, false).await;
  let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM GPU_DATA_ARCHIVE")
    .fetch_one(&pool)
    .await
    .unwrap();
  pool.close().await;
  count
}
