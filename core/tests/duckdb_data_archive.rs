#![cfg(feature = "duckdb-archive")]
//! Whether the native DATA_ARCHIVE family answers what the SQLite family
//! answers.
//!
//! One fixture goes through the production SQLite writer and the production
//! SQLite query, and through candidate -> finalize -> native, and the two
//! answers are compared bit for bit: the bucket grid, the gap convention, the
//! refusals, the stored cell classes and the rows a Retention Period leaves
//! behind. Nothing here asserts a hand-computed series - a hand-computed
//! expectation would only pin what the author believed SQLite does.
//!
//! The fixture is built out of the cases where a reimplementation would be
//! likely to drift: a column that holds both storage classes at once, integers
//! past the point where binary64 stops being exact, negative zero, sub-
//! millisecond instants on either side of SQLite's rounding boundary, date
//! spellings that sort outside the ISO-8601 UTC block, and a stored timestamp
//! SQLite cannot read at all.

mod native_support;

use chrono::{DateTime, Duration, Timelike, Utc};
use hardviz_core::infrastructure::database::archive_queries::{
  ArchiveBucketTimestamp, ArchiveSeriesError, ArchiveSeriesPoint, DataArchiveColumn,
};
use hardviz_core::infrastructure::database::native_database::{
  NativeCancellation, NativeDatabase, NativeDatabaseError, NativeSeriesWindow,
  data_archive as native_data_archive,
};
use hardviz_core::infrastructure::database::{
  archive_queries, db, hardware_archive, migrate,
};
use hardviz_core::persistence::archive_data::{HardwareArchiveRow, HardwareData};
use native_support::{
  NativeFixture, app_migrations, open_pool, read_only, sqlite_epoch_milliseconds_of,
};
use sqlx::{Executor, Row};

/// Every column the Archive screen can ask for, so a column whose storage class
/// or aggregation is special cannot be the one nobody compared.
const COLUMNS: [(DataArchiveColumn, &str); 21] = [
  (DataArchiveColumn::CpuAvg, "cpu_avg"),
  (DataArchiveColumn::CpuMax, "cpu_max"),
  (DataArchiveColumn::CpuMin, "cpu_min"),
  (DataArchiveColumn::RamAvg, "ram_avg"),
  (DataArchiveColumn::RamMax, "ram_max"),
  (DataArchiveColumn::RamMin, "ram_min"),
  (DataArchiveColumn::CpuTemperatureAvg, "cpu_temperature_avg"),
  (DataArchiveColumn::CpuTemperatureMax, "cpu_temperature_max"),
  (DataArchiveColumn::CpuTemperatureMin, "cpu_temperature_min"),
  (DataArchiveColumn::CpuPowerAvg, "cpu_power_avg"),
  (DataArchiveColumn::CpuPowerMax, "cpu_power_max"),
  (DataArchiveColumn::CpuPowerMin, "cpu_power_min"),
  (DataArchiveColumn::GpuPowerAvg, "gpu_power_avg"),
  (DataArchiveColumn::GpuPowerMax, "gpu_power_max"),
  (DataArchiveColumn::GpuPowerMin, "gpu_power_min"),
  (DataArchiveColumn::AnePowerAvg, "ane_power_avg"),
  (DataArchiveColumn::AnePowerMax, "ane_power_max"),
  (DataArchiveColumn::AnePowerMin, "ane_power_min"),
  (DataArchiveColumn::PackagePowerAvg, "package_power_avg"),
  (DataArchiveColumn::PackagePowerMax, "package_power_max"),
  (DataArchiveColumn::PackagePowerMin, "package_power_min"),
];

/// The columns a finalized archive holds as `UNION(i BIGINT, r DOUBLE)`,
/// because their SQLite declaration is INTEGER and their writer binds
/// `Option<f32>`.
const TAGGED_COLUMNS: [&str; 6] = [
  "cpu_avg", "cpu_max", "cpu_min", "ram_avg", "ram_max", "ram_min",
];

#[tokio::test]
async fn the_native_data_archive_family_reproduces_the_sqlite_family() {
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
    let expected = u64::from(table.name == "DATA_ARCHIVE");
    assert_eq!(table.unconvertible_timestamps, expected, "{}", table.name);
  }
  let database = fixture.open().await;

  compare_every_series(&database).await;
  compare_an_averaged_group_past_binary64(&database).await;
  compare_the_refusals(&database).await;
  compare_an_unreadable_timestamp(&database).await;
  compare_the_epoch_bucket_beside_an_unreadable_stamp(&database).await;

  // The write path, after the queries: a row written through each writer must
  // be the same row, down to the storage class and the derived query key.
  let write_instant = (Utc::now() - Duration::days(1))
    .with_nanosecond(500_000)
    .unwrap();
  let written = parity_row();
  hardware_archive::insert(written.clone(), write_instant)
    .await
    .unwrap();
  native_data_archive::insert(
    &database,
    NativeCancellation::new(),
    written,
    write_instant,
  )
  .await
  .unwrap();

  // A NaN reading beside an ordinary one, through both writers, so the gap
  // SQLite stores for it can be compared rather than assumed.
  let nan_minute = write_the_nan_minute(&database).await;

  // The same Retention Period through both delete paths. The legacy 2026-09-01
  // rows are older than any plausible run date, the row just written is a day
  // old, and the unreadable 2099 stamp is kept by both because both compare the
  // bound as text.
  let before = sqlite_row_count(&fixture).await;
  hardware_archive::delete_old_data(7).await.unwrap();
  let expected_deleted = before - sqlite_row_count(&fixture).await;
  assert!(
    expected_deleted >= 1,
    "the Retention Period must bisect the fixture"
  );
  let deleted =
    native_data_archive::delete_old_data(&database, NativeCancellation::new(), 7)
      .await
      .unwrap();
  assert_eq!(i64::try_from(deleted).unwrap(), expected_deleted);

  compare_the_nan_minute(&database, nan_minute).await;

  compare_the_surviving_rows(&fixture, database).await;
}

/// Rows written by the production writer, so the stored timestamp text is
/// whatever sqlx actually produces rather than whatever a test spells.
///
/// Two rows share one instant (a write cycle that retried), and two more sit on
/// either side of the half-millisecond SQLite rounds up.
async fn seed_through_the_production_writer() {
  let instants: [(&str, f32); 5] = [
    ("2026-09-01T00:00:00Z", 1.0),
    ("2026-09-01T00:00:30.125Z", 2.0),
    ("2026-09-01T00:00:30.125Z", 3.0),
    ("2026-09-01T00:02:00.000499Z", 4.0),
    ("2026-09-01T00:03:00.000500Z", 5.0),
  ];
  for (text, seed) in instants {
    let instant: DateTime<Utc> = text.parse().unwrap();
    hardware_archive::insert(written_row(seed), instant)
      .await
      .unwrap();
  }

  // The epoch itself, which is the bucket an unreadable stamp collapses onto
  // in both engines. See `compare_the_epoch_bucket_beside_an_unreadable_stamp`.
  hardware_archive::insert(written_row(7.0), DateTime::from_timestamp(0, 0).unwrap())
    .await
    .unwrap();

  // One row placed relative to the clock rather than on a fixed date, so the
  // Retention Period compared below bisects the fixture whenever the suite
  // runs. Five years back keeps it clear of every range compared above.
  hardware_archive::insert(written_row(6.0), Utc::now() - Duration::days(1_826))
    .await
    .unwrap();
}

/// Rows a released build could have left behind, which the writer cannot
/// produce today: integer storage classes in the tagged columns, values past
/// binary64's exact range, negative zero, an all-NULL minute, and spellings
/// `strftime` reads differently from how byte comparison sorts them - including
/// one it cannot read at all.
async fn seed_legacy_rows() {
  let pool = db::get_pool().await.unwrap();
  pool
    .execute(
      r#"
      INSERT INTO DATA_ARCHIVE
        (cpu_avg,cpu_max,cpu_min,ram_avg,ram_max,ram_min,
         timestamp,cpu_temperature_avg,cpu_power_avg)
      VALUES
        (3,9007199254740993,-9007199254740993,
         5,9223372036854775807,0,'2026-09-01T00:00:10Z',40.5,-0.0),
        (0.5,0.5,0.5,1.25,2.5,-0.0,'2026-09-01T00:00:20Z',-0.0,1.5),
        (NULL,NULL,NULL,NULL,NULL,NULL,'2026-09-01T00:00:40Z',NULL,NULL),
        (7,3.5,2,8,4.5,1,'2026-09-01 00:01:10',12.0,3.0),
        (1,2,3,4,5,6,'2026-09-01T09:01:20+09:00',13.0,4.0),
        (2,3,4,5,6,7,'2026-09-01T00:01:30.500Z',14.0,5.0),
        (8,9,10,11,12,13,'2099-13-45T99:99:99+00:00',15.0,6.0),
        -- The averaged-divergence minute, deliberately outside every range
        -- compared above. See `an_averaged_group_past_binary64_is_where_the_engines_part`.
        (9007199254740993,1,1,1,1,1,'2026-09-02T00:00:00Z',1.0,1.0),
        (0.5,1,1,1,1,1,'2026-09-02T00:00:01Z',1.0,1.0),
        (1.0,1,1,1,1,1,'2026-09-02T00:00:02Z',1.0,1.0);
      "#,
    )
    .await
    .unwrap();
}

/// Every column, over ranges chosen for their endpoints, at bucket widths that
/// put the fixture's instants in one group and in separate groups, with the
/// bucket stamped at both ends.
async fn compare_every_series(database: &NativeDatabase) {
  // Endpoints are compared as bytes by both engines, so the interesting ranges
  // are the ones that land exactly on a stored spelling, just inside a
  // fractional one, and outside the ISO-8601 UTC block entirely.
  let ranges: [(&str, &str); 5] = [
    ("2026-08-31T23:59:00Z", "2026-09-01T00:10:00Z"),
    ("2026-09-01T00:00:00Z", "2026-09-01T00:05:00Z"),
    ("2026-09-01T00:00:30.125Z", "2026-09-01T00:01:30.500Z"),
    ("2026-09-01T00:00:00Z", "2026-09-01T00:00:00Z"),
    ("2027-01-01T00:00:00Z", "2027-01-01T00:01:00Z"),
  ];

  // 2026-09-01T00:00:00Z, the minute holding the integer 2^53+1 beside
  // binary64 readings.
  const MIXED_GROUP_BUCKET: i64 = 1_788_220_800_000;

  let mut compared = 0_usize;
  let mut mixed_group_max = None;
  for (column, name) in COLUMNS {
    for (start, end) in ranges {
      let start: DateTime<Utc> = start.parse().unwrap();
      let end: DateTime<Utc> = end.parse().unwrap();
      for width in [1_000_i64, 60_000, 300_000] {
        for bucket in [ArchiveBucketTimestamp::Start, ArchiveBucketTimestamp::End] {
          let expected = archive_queries::select_data_archive_series(
            column, &start, &end, width, bucket,
          )
          .await
          .unwrap();
          let actual = native_data_archive::select_data_archive_series(
            database,
            NativeCancellation::new(),
            column,
            window(&start, &end, width, bucket),
          )
          .await
          .unwrap();
          assert_eq!(
            bits(&actual),
            bits(&expected),
            "{name} over {start}..{end} at {width}ms, bucket {bucket:?}"
          );
          compared += 1;
          // Ranges whose endpoints exclude the whole minute answer it as a
          // gap, so the recorded value is the one from a range that covers it.
          let recorded = (name == "cpu_max" && width == 60_000)
            .then(|| {
              expected
                .iter()
                .find(|point| point.timestamp == MIXED_GROUP_BUCKET)
                .and_then(|point| point.value)
            })
            .flatten();
          mixed_group_max = recorded.or(mixed_group_max);
        }
      }
    }
  }
  assert_eq!(compared, COLUMNS.len() * ranges.len() * 3 * 2);

  // The mixed-class group, named rather than left implicit: the first minute
  // holds an integer 2^53+1 beside binary64 readings, and both engines answer
  // with the same rounded double because both cast before aggregating. An
  // engine that compared the union's members exactly would return the integer.
  assert_eq!(
    mixed_group_max.map(f64::to_bits),
    Some(9_007_199_254_740_992.0_f64.to_bits()),
    "the mixed-class minute must round through the cast, not stay exact"
  );
}

/// Where the two engines stop agreeing, measured rather than assumed.
///
/// `cpu_avg` is declared INTEGER, so a legacy row can hold an integer past
/// 2^53 - and averaging one of those with ordinary readings is exactly the case
/// `duckdb_avg_compatibility` pins for Process Stats: SQLite's `avg()` has used
/// Kahan-Babuska-Neumaier compensated summation since 3.43 and recovers a sum
/// plain binary64 accumulation loses, while DuckDB's `AVG` does not.
///
/// The minute this reads is seeded apart from every range compared above, so
/// the family comparison stays a comparison rather than a list of exceptions.
/// Today's writers cannot reach it: `cpu_avg` is written from `Option<f32>`, so
/// a group needs a row no released build produced. This is the boundary, named,
/// and it is what says the parity above is bounded rather than universal.
async fn compare_an_averaged_group_past_binary64(database: &NativeDatabase) {
  let start: DateTime<Utc> = "2026-09-02T00:00:00Z".parse().unwrap();
  let end: DateTime<Utc> = "2026-09-02T00:00:30Z".parse().unwrap();
  let expected = archive_queries::select_data_archive_series(
    DataArchiveColumn::CpuAvg,
    &start,
    &end,
    60_000,
    ArchiveBucketTimestamp::Start,
  )
  .await
  .unwrap();
  let actual = native_data_archive::select_data_archive_series(
    database,
    NativeCancellation::new(),
    DataArchiveColumn::CpuAvg,
    window(&start, &end, 60_000, ArchiveBucketTimestamp::Start),
  )
  .await
  .unwrap();

  assert_eq!(expected.len(), 1);
  assert_eq!(actual.len(), 1);
  assert_eq!(actual[0].timestamp, expected[0].timestamp);
  let expected_bits = i128::from(expected[0].value.unwrap().to_bits());
  let actual_bits = i128::from(actual[0].value.unwrap().to_bits());
  // Two ulps for this group, as measured. The number is not the point - that
  // the answers differ at all is - but pinning it means an engine that changes
  // its summation shows up here as a change instead of as silent drift.
  assert_eq!(
    (expected_bits - actual_bits).abs(),
    2,
    "the measured divergence; update this test if an engine changes its \
     summation"
  );
}

/// The three refusals that never reach an engine, so a caller cannot tell the
/// backends apart by how a bad request fails.
async fn compare_the_refusals(database: &NativeDatabase) {
  let start: DateTime<Utc> = "2026-09-01T00:00:00Z".parse().unwrap();
  let end: DateTime<Utc> = "2026-09-02T00:00:00Z".parse().unwrap();

  // start after end
  assert!(matches!(
    refusal(database, &end, &start, 60_000).await,
    (
      ArchiveSeriesError::InvalidTimeRange,
      ArchiveSeriesError::InvalidTimeRange
    )
  ));
  // a bucket width no grid can be built from
  assert!(matches!(
    refusal(database, &start, &end, 0).await,
    (
      ArchiveSeriesError::InvalidBucketWidth,
      ArchiveSeriesError::InvalidBucketWidth
    )
  ));
  // more points than the Archive screen will render
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
  let expected = archive_queries::select_data_archive_series(
    DataArchiveColumn::CpuAvg,
    start,
    end,
    width,
    ArchiveBucketTimestamp::Start,
  )
  .await
  .unwrap_err();
  let actual = native_data_archive::select_data_archive_series(
    database,
    NativeCancellation::new(),
    DataArchiveColumn::CpuAvg,
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

/// A stored timestamp SQLite cannot read as an instant - a spelling no current
/// writer produces, but one a released build could have left behind - has no
/// derived key, and therefore no bucket, in either engine.
///
/// Both answer the request; neither shows the row. SQLite groups it under a
/// NULL bucket that decodes to 0 and falls outside every range a caller can
/// ask for, and the native query reads the same NULL the same way. The rest of
/// the row is not wrong and is not hidden: every other query still returns it,
/// and how many such rows an archive holds is reported once, at conversion
/// time, as `NativeTableReport::unconvertible_timestamps`.
async fn compare_an_unreadable_timestamp(database: &NativeDatabase) {
  let start: DateTime<Utc> = "2099-01-01T00:00:00Z".parse().unwrap();
  let end: DateTime<Utc> = "2100-01-01T00:00:00Z".parse().unwrap();

  // The row is inside the range by the same byte comparison the query uses, and
  // SQLite cannot read it as an instant. Both halves matter: without the first
  // the series below would be empty for an ordinary reason.
  let pool = db::get_pool().await.unwrap();
  let inside: Vec<(String, Option<i64>)> = sqlx::query(
    "SELECT CAST(timestamp AS TEXT), CAST(strftime('%s', timestamp) AS INTEGER) \
     FROM DATA_ARCHIVE WHERE timestamp BETWEEN ? AND ?",
  )
  .bind("2099-01-01T00:00:00.000Z")
  .bind("2100-01-01T00:00:00.000Z")
  .fetch_all(&pool)
  .await
  .unwrap()
  .into_iter()
  .map(|row| (row.get(0), row.get(1)))
  .collect();
  assert_eq!(inside, vec![("2099-13-45T99:99:99+00:00".to_owned(), None)]);

  for bucket in [ArchiveBucketTimestamp::Start, ArchiveBucketTimestamp::End] {
    let expected = archive_queries::select_data_archive_series(
      DataArchiveColumn::CpuAvg,
      &start,
      &end,
      86_400_000,
      bucket,
    )
    .await
    .unwrap();
    let actual = native_data_archive::select_data_archive_series(
      database,
      NativeCancellation::new(),
      DataArchiveColumn::CpuAvg,
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

/// The one bucket where an unreadable stamp is not invisible: bucket 0.
///
/// Both engines answer an unreadable row under a bucket that arrives as
/// timestamp 0 - SQLite because `strftime` returns NULL and sqlx decodes that
/// NULL as 0, the native query because it reads the same NULL the same way. A
/// row stored exactly at the epoch produces a *second*, legitimate group at
/// bucket 0, and `fill_archive_series` keeps whichever of the two the engine
/// returned first. SQLite sorts NULL first; DuckDB sorts it last. Without
/// `NULLS FIRST` on the native query the two engines therefore answer different
/// values for this one bucket, which is what this range is here to catch.
///
/// The range has to hold both stored spellings under the query's byte
/// comparison - `1970-01-01T00:00:00+00:00` and `2099-13-45T99:99:99+00:00` -
/// so it spans them, at a bucket width wide enough to stay inside the point
/// limit.
async fn compare_the_epoch_bucket_beside_an_unreadable_stamp(database: &NativeDatabase) {
  let start: DateTime<Utc> = "1969-01-01T00:00:00Z".parse().unwrap();
  let end: DateTime<Utc> = "2100-01-01T00:00:00Z".parse().unwrap();
  // Thirty days, which puts roughly 1,600 points inside the limit.
  let width = 2_592_000_000_i64;

  // `cpu_avg` is deliberately not among these. At this width every row of the
  // fixture shares one bucket, including the 2^53 integer that
  // `compare_an_averaged_group_past_binary64` exists to pin - the engines part
  // there for a reason that has nothing to do with ordering, and that test owns
  // it. `ram_avg` keeps a tagged column under an averaging aggregate here
  // without crossing that boundary.
  for (column, name) in [
    (DataArchiveColumn::RamAvg, "ram_avg"),
    (DataArchiveColumn::CpuMax, "cpu_max"),
    (DataArchiveColumn::CpuMin, "cpu_min"),
    (DataArchiveColumn::CpuTemperatureAvg, "cpu_temperature_avg"),
  ] {
    for bucket in [ArchiveBucketTimestamp::Start, ArchiveBucketTimestamp::End] {
      let expected =
        archive_queries::select_data_archive_series(column, &start, &end, width, bucket)
          .await
          .unwrap();
      let actual = native_data_archive::select_data_archive_series(
        database,
        NativeCancellation::new(),
        column,
        window(&start, &end, width, bucket),
      )
      .await
      .unwrap();
      assert_eq!(bits(&actual), bits(&expected), "{name}, bucket {bucket:?}");

      // The bucket has to carry a value, or the two engines would agree only
      // because neither found the epoch row.
      let epoch_bucket = expected
        .iter()
        .find(|point| point.timestamp == 0)
        .unwrap_or_else(|| panic!("{name}: the epoch bucket must be in the series"));
      assert!(
        epoch_bucket.value.is_some(),
        "{name}: the epoch bucket must carry the reading SQLite kept"
      );
    }
  }
}

/// What both engines hold after the same Retention Period: the same rows, and
/// for the row each writer just wrote, the same cell classes, the same binary64
/// bits, the same timestamp bytes and the same derived query key.
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

  let expected_ids: Vec<i64> = sqlx::query("SELECT id FROM DATA_ARCHIVE ORDER BY id")
    .fetch_all(&pool)
    .await
    .unwrap()
    .into_iter()
    .map(|row| row.get(0))
    .collect();
  let actual_ids: Vec<i64> = connection
    .prepare("SELECT id FROM DATA_ARCHIVE ORDER BY id")
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

  // The stored timestamp bytes, and the key the native side has to answer
  // range queries with. The SQLite side has no such column, so the comparison
  // runs the production adapter over its stored text - which is exactly what
  // finalization would have done to this row had it been converted.
  let expected_stamp: (String, i64) = sqlx::query(&format!(
    "SELECT CAST(timestamp AS TEXT), {} FROM DATA_ARCHIVE WHERE id = ?",
    sqlite_epoch_milliseconds_of("timestamp")
  ))
  .bind(written_id)
  .fetch_one(&pool)
  .await
  .map(|row| (row.get(0), row.get(1)))
  .unwrap();
  let actual_stamp: (String, i64) = connection
    .query_row(
      "SELECT \"timestamp\", __hv_timestamp_epoch_ms FROM DATA_ARCHIVE WHERE id = ?",
      [written_id],
      |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .unwrap();
  assert_eq!(actual_stamp, expected_stamp);
  assert!(
    actual_stamp.0.ends_with(".000500+00:00"),
    "the sub-millisecond instant must survive as written: {}",
    actual_stamp.0
  );

  // The class each cell was stored in, and its exact value in that class. The
  // class is the claim: an INTEGER-declared column takes integral readings as
  // integers under SQLite's affinity rule, and a native row that recorded them
  // as reals would read back the same numbers while being a different row.
  for (&written_id, (_, name)) in inspected
    .iter()
    .flat_map(|id| std::iter::repeat(id).zip(COLUMNS))
  {
    let expected: (String, Option<i64>, Option<f64>) = sqlx::query(&format!(
      "SELECT typeof({name}), \
       CASE WHEN typeof({name}) = 'integer' THEN {name} END, \
       CASE WHEN typeof({name}) = 'real' THEN {name} END \
       FROM DATA_ARCHIVE WHERE id = ?"
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
               FROM DATA_ARCHIVE WHERE id = ?"
            ),
            [written_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
          )
          .unwrap()
      } else {
        connection
          .query_row(
            &format!(
              "SELECT CASE WHEN {name} IS NULL THEN NULL ELSE 'r' END, \
               NULL::BIGINT, {name} FROM DATA_ARCHIVE WHERE id = ?"
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
    // No cell on either side is a stored NaN: SQLite cannot hold one, so a
    // native row that did would be a row the source could not have produced.
    assert!(!expected.2.is_some_and(f64::is_nan), "{name}");
    assert!(!actual.2.is_some_and(f64::is_nan), "{name}");
  }
  assert!(
    TAGGED_COLUMNS.iter().count() == 6,
    "the affinity rule applies to exactly the INTEGER-declared columns"
  );
  pool.close().await;
  // Before the fixture's temporary directory goes, so the file is not removed
  // out from under an open handle.
  drop(connection);
}

/// One minute written through both writers: an ordinary row, then a row whose
/// CPU and temperature readings are NaN.
///
/// The pair is what makes the comparison mean something. A bucket holding only
/// the NaN row would agree trivially once both engines store a gap; a bucket
/// holding a value beside it is where an engine that kept the NaN as a number
/// would answer NaN for `AVG`, `MIN` and `MAX` while the other answers the
/// ordinary reading.
async fn write_the_nan_minute(database: &NativeDatabase) -> DateTime<Utc> {
  let minute = (Utc::now() - Duration::days(2))
    .with_second(0)
    .unwrap()
    .with_nanosecond(0)
    .unwrap();
  for (offset, row) in [
    (Duration::seconds(30), complete_row()),
    (Duration::seconds(31), nan_row()),
  ] {
    let instant = minute + offset;
    hardware_archive::insert(row.clone(), instant)
      .await
      .unwrap();
    native_data_archive::insert(database, NativeCancellation::new(), row, instant)
      .await
      .unwrap();
  }
  minute
}

/// What each engine answers for the minute holding the NaN row - one tagged
/// column per aggregation and one REAL-affinity column per aggregation, which
/// is every path a NaN can take into this table.
async fn compare_the_nan_minute(database: &NativeDatabase, minute: DateTime<Utc>) {
  // Endpoints a second away from either stored stamp, so the byte comparison
  // includes both rows rather than tying with one of their spellings.
  let start = minute;
  let end = minute + Duration::seconds(59);
  for (column, name) in [
    (DataArchiveColumn::CpuAvg, "cpu_avg"),
    (DataArchiveColumn::CpuMax, "cpu_max"),
    (DataArchiveColumn::CpuMin, "cpu_min"),
    (DataArchiveColumn::CpuTemperatureAvg, "cpu_temperature_avg"),
    (DataArchiveColumn::CpuTemperatureMax, "cpu_temperature_max"),
    (DataArchiveColumn::CpuTemperatureMin, "cpu_temperature_min"),
  ] {
    for bucket in [ArchiveBucketTimestamp::Start, ArchiveBucketTimestamp::End] {
      let expected =
        archive_queries::select_data_archive_series(column, &start, &end, 60_000, bucket)
          .await
          .unwrap();
      let actual = native_data_archive::select_data_archive_series(
        database,
        NativeCancellation::new(),
        column,
        window(&start, &end, 60_000, bucket),
      )
      .await
      .unwrap();
      assert_eq!(bits(&actual), bits(&expected), "{name}, bucket {bucket:?}");
      // The ordinary row has to be visible, or the agreement above would only
      // say that both engines found nothing.
      let answered = expected.iter().filter_map(|point| point.value).next();
      assert_eq!(
        answered.map(f64::is_nan),
        Some(false),
        "{name} must answer the ordinary reading, not a NaN or a gap"
      );
    }
  }
}

/// A row whose readings include the values a writer can produce but a test
/// would not think to spell: negative zero, the extremes of `f32`, and absent
/// readings beside present ones.
fn parity_row() -> HardwareArchiveRow {
  HardwareArchiveRow {
    cpu: data(Some(-0.0), Some(f32::MAX), Some(f32::MIN_POSITIVE)),
    memory: data(Some(0.1), None, Some(-0.0)),
    cpu_temperature: data(None, None, None),
    cpu_power: data(Some(f32::MIN), Some(0.0), None),
    gpu_power: data(Some(1.5), Some(2.5), Some(0.5)),
    ane_power: data(None, Some(-0.0), None),
    package_power: data(Some(12.75), Some(13.5), Some(11.25)),
  }
}

/// The NaN row's companion: every reading present, so each column compared
/// over that minute has an ordinary answer a stored NaN would have displaced.
fn complete_row() -> HardwareArchiveRow {
  let present = || data(Some(4.0), Some(6.0), Some(2.0));
  HardwareArchiveRow {
    cpu: present(),
    memory: present(),
    cpu_temperature: present(),
    cpu_power: present(),
    gpu_power: present(),
    ane_power: present(),
    package_power: present(),
  }
}

/// NaN in both the INTEGER-declared columns and the REAL-declared ones, beside
/// ordinary readings in the rest so the row is not trivially empty.
fn nan_row() -> HardwareArchiveRow {
  HardwareArchiveRow {
    cpu: data(Some(f32::NAN), Some(f32::NAN), Some(-f32::NAN)),
    memory: data(Some(f32::NAN), Some(7.5), Some(f32::NAN)),
    cpu_temperature: data(Some(f32::NAN), Some(f32::NAN), Some(-f32::NAN)),
    cpu_power: data(Some(f32::NAN), Some(2.25), None),
    // Infinity is a reading, not a gap, and must survive as one.
    gpu_power: data(Some(f32::INFINITY), Some(f32::NEG_INFINITY), Some(0.5)),
    ane_power: data(None, None, None),
    package_power: data(Some(1.0), Some(2.0), Some(3.0)),
  }
}

fn written_row(seed: f32) -> HardwareArchiveRow {
  HardwareArchiveRow {
    cpu: data(Some(seed), Some(seed * 2.0), Some(seed / 4.0)),
    memory: data(Some(seed + 0.25), Some(seed + 1.5), Some(seed - 0.75)),
    cpu_temperature: data(Some(40.0 + seed), Some(45.0 + seed), None),
    cpu_power: data(Some(seed * 1.5), None, Some(seed * 0.5)),
    gpu_power: data(None, None, None),
    ane_power: data(Some(0.125 * seed), Some(0.25 * seed), Some(0.0)),
    package_power: data(Some(seed * 3.0), Some(seed * 4.0), Some(seed)),
  }
}

fn data(avg: Option<f32>, max: Option<f32>, min: Option<f32>) -> HardwareData {
  HardwareData { avg, max, min }
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
  let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM DATA_ARCHIVE")
    .fetch_one(&pool)
    .await
    .unwrap();
  pool.close().await;
  count
}
