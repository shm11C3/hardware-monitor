#![cfg(feature = "duckdb-archive")]
//! Finalization of a #2088 candidate into the stable native schema, and the
//! behavior of the blocking owner that serves the result.

mod native_support;

use std::path::Path;
use std::time::Duration;

use duckdb::Connection;
use hardviz_core::infrastructure::database::native_database::{
  NativeCancellation, NativeDatabaseError, NativeDatabaseOptions, process_stats,
};
use hardviz_core::persistence::archive_data::ProcessStatData;
use native_support::{
  NativeFixture, app_native_schema, file_hash, read_only, read_write,
  sqlite_epoch_milliseconds_of,
};
use sqlx::{Executor, Row, SqlitePool};

const STORAGE_ID: &str = "storage:hmac-sha256:v1:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[tokio::test]
async fn finalizes_every_domain_table_preserving_classes_ids_and_multiplicity() {
  let fixture = NativeFixture::new();
  let pool = fixture.migrated_pool().await;
  seed_domain_tables(&pool).await;
  pool.close().await;
  let source_hash = file_hash(&fixture.source);

  fixture.build_candidate().await.unwrap();
  let candidate_hash = file_hash(&fixture.candidate);
  let report = fixture.try_finalize().await.unwrap();

  assert_eq!(report.state, "finalized_unselected");
  assert_eq!(
    report.schema_version,
    app_native_schema::NATIVE_SCHEMA_VERSION
  );
  assert_eq!(report.source_schema_sha256.len(), 64);
  assert_eq!(report.tables.len(), 15);
  assert!(report.finalized_bytes > 0);
  for table in &report.tables {
    assert_eq!(table.candidate_rows, table.copied_rows, "{}", table.name);
    assert_eq!(table.copied_rows, table.reopened_rows, "{}", table.name);
    assert_eq!(table.copied_digest, table.reopened_digest, "{}", table.name);
    assert_eq!(table.copied_digest.len(), 64, "{}", table.name);
    // Every stamp in this fixture is one SQLite can read, so the conversion
    // has nothing to report.
    assert_eq!(table.unconvertible_timestamps, 0, "{}", table.name);
  }
  assert_eq!(
    report.tables.iter().map(|t| t.copied_rows).sum::<u64>(),
    report.total_rows
  );
  // The `_sqlx_migrations` and `sqlite_sequence` rows the candidate carries are
  // provenance and allocator state, not domain rows, so they are not copied.
  let process = report
    .tables
    .iter()
    .find(|table| table.name == "PROCESS_STATS")
    .unwrap();
  assert_eq!(process.copied_rows, 6);

  // Neither input was touched, and no work directory survived.
  assert_eq!(file_hash(&fixture.source), source_hash);
  assert_eq!(file_hash(&fixture.candidate), candidate_hash);
  assert!(fixture.work_directories().is_empty());

  let connection = read_only(&fixture.finalized);
  assert_eq!(storage_header_version(&fixture.finalized), 64);
  let recorded_storage_version: String = connection
    .query_row(
      "SELECT storage_version FROM __hv_native_metadata",
      [],
      |row| row.get(0),
    )
    .unwrap();
  let engine_storage_version: String = connection
    .query_row(
      "SELECT tags['storage_version'] FROM duckdb_databases() \
       WHERE database_name = current_database()",
      [],
      |row| row.get(0),
    )
    .unwrap();
  assert_eq!(recorded_storage_version, "v1.0.0+");
  assert_eq!(engine_storage_version, recorded_storage_version);
  // Tagged unions keep the integer/real distinction and the exact binary64 bits.
  assert_eq!(
    cpu_avg_union(&connection, i64::MIN),
    (Some("i".to_owned()), Some(i64::MIN), None)
  );
  assert_eq!(
    cpu_avg_union(&connection, i64::MAX),
    (Some("i".to_owned()), Some(i64::MAX), None)
  );
  let real = cpu_avg_union(&connection, 0);
  assert_eq!(real.0.as_deref(), Some("r"));
  assert_eq!(
    real.2.map(f64::to_bits),
    Some(f64::from(25.25_f32).to_bits())
  );
  assert_eq!(cpu_avg_union(&connection, -1), (None, None, None));

  // Text bytes survive, including an embedded NUL.
  let gpu_name: String = connection
    .query_row(
      "SELECT gpu_name FROM GPU_DATA_ARCHIVE WHERE id = 2",
      [],
      |row| row.get(0),
    )
    .unwrap();
  assert_eq!(gpu_name.as_bytes(), b"Discrete GPU\0Secondary");

  // Extreme integers and duplicated identity tuples survive unrounded. The
  // `i64::MAX` row keeps its own identity so a whole-range Process query still
  // has an exact integer sum for every group.
  let memory: i64 = connection
    .query_row(
      "SELECT memory_usage FROM PROCESS_STATS WHERE id = 2",
      [],
      |row| row.get(0),
    )
    .unwrap();
  assert_eq!(memory, i64::MAX);
  let duplicates: i64 = connection
    .query_row(
      "SELECT COUNT(*) FROM PROCESS_STATS WHERE pid = 4000 AND process_name = 'renderer'",
      [],
      |row| row.get(0),
    )
    .unwrap();
  assert_eq!(duplicates, 3);

  // A REAL-declared column stays DOUBLE rather than becoming a union.
  let declared: Vec<(String, String)> = connection
    .prepare(
      "SELECT column_name, data_type FROM information_schema.columns \
       WHERE table_name = 'DATA_ARCHIVE' AND column_name IN ('cpu_avg','cpu_temperature_avg') \
       ORDER BY column_name",
    )
    .unwrap()
    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
    .unwrap()
    .collect::<Result<Vec<_>, _>>()
    .unwrap();
  assert_eq!(
    declared,
    vec![
      ("cpu_avg".to_owned(), "UNION(i BIGINT, r DOUBLE)".to_owned()),
      ("cpu_temperature_avg".to_owned(), "DOUBLE".to_owned()),
    ]
  );
}

#[tokio::test]
async fn refuses_a_native_file_when_recorded_storage_version_disagrees() {
  let fixture = seeded_native().await;
  {
    let connection = read_write(&fixture.finalized);
    connection
      .execute(
        "UPDATE __hv_native_metadata SET storage_version = 'v1.2.0+'",
        [],
      )
      .unwrap();
    connection.execute_batch("CHECKPOINT").unwrap();
  }

  let error = fixture
    .try_open(app_native_schema::NATIVE_SCHEMA_VERSION)
    .await
    .unwrap_err();
  assert!(
    matches!(error, NativeDatabaseError::StorageVersionMismatch { .. }),
    "{error:?}"
  );
  assert!(matches!(
    fixture.authority_state(),
    hardviz_core::infrastructure::database::native_database::AuthorityState::Inconsistent {
      reason: hardviz_core::infrastructure::database::native_database::AuthorityInconsistency::StorageVersionMismatch,
      ..
    }
  ));
}

#[tokio::test]
async fn refuses_legacy_native_metadata_with_an_explicit_compatibility_error() {
  let fixture = seeded_native().await;
  {
    let connection = read_write(&fixture.finalized);
    connection
      .execute(
        "ALTER TABLE __hv_native_metadata DROP COLUMN storage_version",
        [],
      )
      .unwrap();
    connection.execute_batch("CHECKPOINT").unwrap();
  }

  let error = fixture
    .try_open(app_native_schema::NATIVE_SCHEMA_VERSION)
    .await
    .unwrap_err();
  assert!(
    matches!(error, NativeDatabaseError::StorageVersionMetadataMissing),
    "{error:?}"
  );
  assert!(matches!(
    fixture.authority_state(),
    hardviz_core::infrastructure::database::native_database::AuthorityState::Inconsistent {
      reason: hardviz_core::infrastructure::database::native_database::AuthorityInconsistency::StorageVersionMetadataMissing,
      ..
    }
  ));
}

#[test]
fn duckdb_lock_bumps_require_a_storage_format_review() {
  let lock = include_str!("../../Cargo.lock").replace("\r\n", "\n");
  assert!(
    lock.contains("name = \"duckdb\"\nversion = \"1.10505.0\""),
    "a DuckDB crate bump requires reviewing the pinned native storage format"
  );
  assert!(
    lock.contains("name = \"libduckdb-sys\"\nversion = \"1.10505.0\""),
    "a libduckdb-sys bump requires reviewing the pinned native storage format"
  );
}

#[tokio::test]
async fn derived_epoch_keys_equal_the_sqlite_expression_including_the_stamps_it_refuses()
{
  let fixture = NativeFixture::new();
  let pool = fixture.migrated_pool().await;
  seed_domain_tables(&pool).await;
  // Every spelling a writer or an older build could have stored, plus one
  // SQLite cannot read as an instant at all.
  pool
    .execute(
      r#"
      INSERT INTO AMBIENT_ARCHIVE(source,temperature,humidity,timestamp) VALUES
        ('spelling:z',20.0,NULL,'2026-09-01T00:00:00Z'),
        ('spelling:offset',20.0,NULL,'2026-09-01T09:00:00+09:00'),
        ('spelling:millis',20.0,NULL,'2026-09-01T00:00:00.125+00:00'),
        ('spelling:micros',20.0,NULL,'2026-09-01T00:00:00.000125+00:00'),
        ('spelling:space',20.0,NULL,'2026-09-01 00:00:00'),
        ('spelling:unreadable',20.0,NULL,'not a timestamp');
      INSERT INTO FAN_ARCHIVE(id,source,rpm,timestamp) VALUES
        (10,'CPU Fan',1200,'2026-09-01T00:00:00.500Z'),
        (11,'CPU Fan',1200,'');
      "#,
    )
    .await
    .unwrap();

  // The oracle: the production epoch adapter run against the SQLite source.
  let expected: Vec<(i64, Option<i64>)> = sqlx::query(&format!(
    "SELECT id, {} FROM AMBIENT_ARCHIVE ORDER BY id",
    sqlite_epoch_milliseconds_of("timestamp")
  ))
  .fetch_all(&pool)
  .await
  .unwrap()
  .into_iter()
  .map(|row| (row.get(0), row.get(1)))
  .collect();
  let expected_fan: Vec<(i64, Option<i64>)> = sqlx::query(&format!(
    "SELECT id, {} FROM FAN_ARCHIVE ORDER BY id",
    sqlite_epoch_milliseconds_of("timestamp")
  ))
  .fetch_all(&pool)
  .await
  .unwrap()
  .into_iter()
  .map(|row| (row.get(0), row.get(1)))
  .collect();
  pool.close().await;
  assert!(expected.iter().any(|(_, value)| value.is_none()));
  assert!(expected_fan.iter().any(|(_, value)| value.is_none()));

  fixture.finalize().await;
  let connection = read_only(&fixture.finalized);
  for (table, expected) in [("AMBIENT_ARCHIVE", expected), ("FAN_ARCHIVE", expected_fan)]
  {
    let actual = connection
      .prepare(&format!(
        "SELECT id, __hv_timestamp_epoch_ms FROM {table} ORDER BY id"
      ))
      .unwrap()
      .query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?))
      })
      .unwrap()
      .collect::<Result<Vec<_>, _>>()
      .unwrap();
    assert_eq!(actual, expected, "{table}");
  }
}

#[tokio::test]
async fn imports_autoincrement_high_water_and_records_rowid_tables() {
  let fixture = NativeFixture::new();
  let pool = fixture.migrated_pool().await;
  seed_domain_tables(&pool).await;
  // A deleted highest row leaves its AUTOINCREMENT high-water mark behind.
  pool
    .execute(
      r#"
      INSERT INTO PROCESS_STATS VALUES
        (900,9999,'deleted',0.0,0,0,'2026-09-02T00:00:00+00:00');
      DELETE FROM PROCESS_STATS WHERE id = 900;
      "#,
    )
    .await
    .unwrap();
  let sequence: i64 =
    sqlx::query_scalar("SELECT seq FROM sqlite_sequence WHERE name = 'PROCESS_STATS'")
      .fetch_one(&pool)
      .await
      .unwrap();
  assert_eq!(sequence, 900);
  pool.close().await;

  fixture.finalize().await;
  let connection = read_only(&fixture.finalized);
  let identities = connection
    .prepare("SELECT table_name, column_name, mode, high_water FROM __hv_native_identities ORDER BY table_name")
    .unwrap()
    .query_map([], |row| {
      Ok((
        row.get::<_, String>(0)?,
        row.get::<_, String>(1)?,
        row.get::<_, String>(2)?,
        row.get::<_, i64>(3)?,
      ))
    })
    .unwrap()
    .collect::<Result<Vec<_>, _>>()
    .unwrap();
  assert_eq!(
    identities,
    vec![
      (
        "AMBIENT_ARCHIVE".into(),
        "id".into(),
        "autoincrement".into(),
        2
      ),
      ("DATA_ARCHIVE".into(), "id".into(), "rowid".into(), 0),
      ("FAN_ARCHIVE".into(), "id".into(), "rowid".into(), 0),
      ("GPU_DATA_ARCHIVE".into(), "id".into(), "rowid".into(), 0),
      (
        "PROCESS_STATS".into(),
        "id".into(),
        "autoincrement".into(),
        900
      ),
      (
        "storage_health_daily_records".into(),
        "id".into(),
        "autoincrement".into(),
        1
      ),
    ]
  );
}

#[tokio::test]
async fn identity_allocation_survives_deletion_and_reopen() {
  let fixture = NativeFixture::new();
  let pool = fixture.migrated_pool().await;
  seed_domain_tables(&pool).await;
  pool.close().await;
  fixture.finalize().await;

  let database = fixture.open().await;
  let stamp = "2026-09-03T00:00:00Z".parse().unwrap();
  process_stats::insert(
    &database,
    NativeCancellation::new(),
    vec![process("first", 1.0)],
    stamp,
  )
  .await
  .unwrap();
  let first = highest_process_id(&database).await;
  assert_eq!(first, 7);

  // Deleting the highest AUTOINCREMENT row must not release its id, and the
  // rowid table beside it must behave the way SQLite's rowid tables do.
  database
    .request_write(NativeCancellation::new(), |context| {
      context.with_transaction(|transaction| {
        transaction
          .connection()
          .execute("DELETE FROM PROCESS_STATS WHERE id >= 6", [])
          .unwrap();
        transaction
          .connection()
          .execute(
            "DELETE FROM DATA_ARCHIVE WHERE id = 9223372036854775807",
            [],
          )
          .unwrap();
        Ok(())
      })
    })
    .await
    .unwrap();

  process_stats::insert(
    &database,
    NativeCancellation::new(),
    vec![process("second", 2.0)],
    stamp,
  )
  .await
  .unwrap();
  assert_eq!(highest_process_id(&database).await, 8);
  database.close().await.unwrap();

  let database = fixture.open().await;
  process_stats::insert(
    &database,
    NativeCancellation::new(),
    vec![process("third", 3.0)],
    stamp,
  )
  .await
  .unwrap();
  assert_eq!(highest_process_id(&database).await, 9);

  // The rowid table reuses the id of the deleted highest row, exactly as
  // SQLite's `INTEGER PRIMARY KEY` allocation does.
  let reused = database
    .request_write(NativeCancellation::new(), |context| {
      context.with_transaction(|transaction| transaction.next_id("DATA_ARCHIVE"))
    })
    .await
    .unwrap();
  assert_eq!(reused, 2);
  database.close().await.unwrap();
}

#[tokio::test]
async fn refuses_an_existing_destination_without_touching_it() {
  let fixture = NativeFixture::new();
  let pool = fixture.migrated_pool().await;
  seed_domain_tables(&pool).await;
  pool.close().await;
  fixture.build_candidate().await.unwrap();

  let sentinel = b"an existing native database must survive";
  std::fs::write(&fixture.finalized, sentinel).unwrap();
  let error = fixture.try_finalize().await.unwrap_err();

  assert!(
    matches!(error, NativeDatabaseError::DestinationExists { .. }),
    "{error:?}"
  );
  assert_eq!(std::fs::read(&fixture.finalized).unwrap(), sentinel);
  assert!(fixture.work_directories().is_empty());
}

#[tokio::test]
async fn refuses_a_cell_the_stable_column_cannot_hold() {
  let fixture = NativeFixture::new();
  let pool = fixture.migrated_pool().await;
  seed_domain_tables(&pool).await;
  // `temperature_max` is written from `Option<i32>`, so the stable schema
  // declares it BIGINT. A REAL cell there is a legacy representation the
  // finalized column cannot hold without dropping the fraction.
  sqlx::query("UPDATE GPU_DATA_ARCHIVE SET temperature_max = ? WHERE id = 2")
    .bind(Some(55.5_f32))
    .execute(&pool)
    .await
    .unwrap();
  let classes: Vec<String> = sqlx::query_scalar(
    "SELECT typeof(temperature_max) FROM GPU_DATA_ARCHIVE ORDER BY id",
  )
  .fetch_all(&pool)
  .await
  .unwrap();
  assert_eq!(classes, ["integer", "real"]);
  pool.close().await;
  fixture.build_candidate().await.unwrap();

  let error = fixture.try_finalize().await.unwrap_err();

  match error {
    NativeDatabaseError::UnrepresentableCell {
      ref table,
      ref column,
      row_ordinal,
      candidate,
      ref destination,
    } => {
      assert_eq!(table, "GPU_DATA_ARCHIVE");
      assert_eq!(column, "temperature_max");
      assert_eq!(row_ordinal, 1);
      assert_eq!(candidate, "a real");
      assert_eq!(destination, "BIGINT");
    }
    other => panic!("unexpected error: {other:?}"),
  }
  assert!(!fixture.finalized.exists());
  assert!(fixture.work_directories().is_empty());
}

/// The stable schema is App-owned and the migration set can move without it.
/// A table the definition forgot would otherwise be dropped silently, which is
/// the loss the missing-column refusal above already rules out.
#[tokio::test]
async fn refuses_a_candidate_table_the_stable_schema_never_declared() {
  // The App list minus one table. Its DDL still runs, so the only difference
  // is that nothing would copy its rows.
  const WITHOUT_HOURLY_SUMMARY: &[&str] = &[
    "DATA_ARCHIVE",
    "GPU_DATA_ARCHIVE",
    "PROCESS_STATS",
    "storage_devices",
    "storage_health_daily_records",
    "cooling_daily_summary",
    "cooling_baseline",
    "AMBIENT_ARCHIVE",
    "FAN_ARCHIVE",
    "cooling_fan_daily_summary",
    "cooling_delta_baseline",
    "cooling_thermal_delta_daily_summary",
    "cooling_covariate_daily_summary",
    "cooling_fan_covariate_daily_summary",
  ];

  let fixture = NativeFixture::new();
  let pool = fixture.migrated_pool().await;
  seed_domain_tables(&pool).await;
  pool.close().await;
  fixture.build_candidate().await.unwrap();

  let mut schema = app_native_schema::get_native_schema();
  schema.tables = WITHOUT_HOURLY_SUMMARY;
  let error =
    hardviz_core::infrastructure::database::native_database::finalize_candidate_database(
      &fixture.candidate,
      &fixture.finalized,
      schema,
    )
    .await
    .unwrap_err();

  match error {
    NativeDatabaseError::SchemaMismatch { ref table, .. } => {
      assert_eq!(table, "cooling_hourly_summary");
    }
    other => panic!("unexpected error: {other:?}"),
  }
  assert!(!fixture.finalized.exists());
  assert!(fixture.work_directories().is_empty());
}

#[tokio::test]
async fn refuses_an_unfinalized_or_incompatible_database() {
  let fixture = NativeFixture::new();
  let pool = fixture.migrated_pool().await;
  seed_domain_tables(&pool).await;
  pool.close().await;
  fixture.finalize().await;

  // The candidate is a valid DuckDB file, but it was never finalized.
  let unfinalized = fixture.directory.path().join("unfinalized.duckdb");
  std::fs::copy(&fixture.candidate, &unfinalized).unwrap();
  let error =
    hardviz_core::infrastructure::database::native_database::NativeDatabase::open(
      &unfinalized,
      NativeDatabaseOptions::new(app_native_schema::NATIVE_SCHEMA_VERSION),
    )
    .await
    .unwrap_err();
  assert!(
    matches!(error, NativeDatabaseError::Unfinalized),
    "{error:?}"
  );

  let error = fixture
    .try_open(app_native_schema::NATIVE_SCHEMA_VERSION + 1)
    .await
    .unwrap_err();
  assert!(
    matches!(
      error,
      NativeDatabaseError::IncompatibleSchema { actual: 1, .. }
    ),
    "{error:?}"
  );

  let missing = fixture.directory.path().join("absent.duckdb");
  let error =
    hardviz_core::infrastructure::database::native_database::NativeDatabase::open(
      &missing,
      NativeDatabaseOptions::new(app_native_schema::NATIVE_SCHEMA_VERSION),
    )
    .await
    .unwrap_err();
  assert!(
    matches!(error, NativeDatabaseError::Unavailable { .. }),
    "{error:?}"
  );
}

#[tokio::test]
async fn requests_after_close_report_closed() {
  let fixture = seeded_native().await;
  let database = fixture.open().await;
  database.close().await.unwrap();
  // Closing twice is a no-op rather than an error.
  database.close().await.unwrap();

  let error = process_stats::select_process_stats(
    &database,
    NativeCancellation::new(),
    String::new(),
    "z".to_owned(),
    false,
  )
  .await
  .unwrap_err();
  assert!(matches!(error, NativeDatabaseError::Closed), "{error:?}");

  let error = process_stats::insert(
    &database,
    NativeCancellation::new(),
    vec![process("after-close", 1.0)],
    "2026-09-03T00:00:00Z".parse().unwrap(),
  )
  .await
  .unwrap_err();
  assert!(matches!(error, NativeDatabaseError::Closed), "{error:?}");
}

#[tokio::test]
async fn a_cancellation_may_be_attached_to_only_one_request() {
  let fixture = seeded_native().await;
  let database = fixture.open().await;
  let cancellation = NativeCancellation::new();
  process_stats::select_process_stats(
    &database,
    cancellation.clone(),
    String::new(),
    "z".to_owned(),
    false,
  )
  .await
  .unwrap();

  let error = process_stats::select_process_stats(
    &database,
    cancellation,
    String::new(),
    "z".to_owned(),
    false,
  )
  .await
  .unwrap_err();
  assert!(
    matches!(error, NativeDatabaseError::CancellationAlreadyUsed),
    "{error:?}"
  );
  database.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_interrupts_a_read_and_the_owner_keeps_serving() {
  let fixture = seeded_native().await;
  let database = fixture.open().await;
  let cancellation = NativeCancellation::new();
  let (started, mut started_rx) = tokio::sync::mpsc::channel::<()>(1);
  let running = database.clone();
  let token = cancellation.clone();
  let long_read = tokio::spawn(async move {
    running
      .request_read(token, move |context| {
        started.blocking_send(()).unwrap();
        context
          .connection()
          .query_row(
            "SELECT max(hash(range)) FROM range(0, 5000000000)",
            [],
            |row| row.get::<_, u64>(0),
          )
          .map_err(|error| NativeDatabaseError::Worker {
            message: error.to_string(),
          })
      })
      .await
  });

  started_rx.recv().await.unwrap();
  tokio::time::sleep(Duration::from_millis(400)).await;
  cancellation.cancel();
  let error = tokio::time::timeout(Duration::from_secs(60), long_read)
    .await
    .expect("the interrupted read must finish")
    .unwrap()
    .unwrap_err();
  assert!(matches!(error, NativeDatabaseError::Cancelled), "{error:?}");

  // The same owner keeps serving both lanes afterwards.
  let before = process_stats::select_process_stats(
    &database,
    NativeCancellation::new(),
    String::new(),
    "z".to_owned(),
    false,
  )
  .await
  .unwrap();
  process_stats::insert(
    &database,
    NativeCancellation::new(),
    vec![process("after-cancellation", 9.5)],
    "2026-09-03T00:00:00Z".parse().unwrap(),
  )
  .await
  .unwrap();
  let after = process_stats::select_process_stats(
    &database,
    NativeCancellation::new(),
    String::new(),
    "z".to_owned(),
    false,
  )
  .await
  .unwrap();
  assert_eq!(after.len(), before.len() + 1);
  database.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reader_snapshot_stays_pinned_while_a_write_commits() {
  let fixture = seeded_native().await;
  let database = fixture.open().await;
  let (started, mut started_rx) = tokio::sync::mpsc::channel::<i64>(1);
  let (release, mut release_rx) = tokio::sync::mpsc::channel::<()>(1);
  let reader = database.clone();
  let pinned = tokio::spawn(async move {
    reader
      .request_read(NativeCancellation::new(), move |context| {
        context.with_transaction(|transaction| {
          let first = count_processes(transaction.connection());
          started.blocking_send(first).unwrap();
          release_rx.blocking_recv().unwrap();
          Ok((first, count_processes(transaction.connection())))
        })
      })
      .await
  });

  let before = started_rx.recv().await.unwrap();
  process_stats::insert(
    &database,
    NativeCancellation::new(),
    vec![process("committed-during-read", 4.5)],
    "2026-09-03T00:00:00Z".parse().unwrap(),
  )
  .await
  .unwrap();
  release.send(()).await.unwrap();

  let (first, second) = pinned.await.unwrap().unwrap();
  assert_eq!(first, before);
  assert_eq!(
    second, before,
    "the pinned snapshot must not see the commit"
  );

  let after = database
    .request_read(NativeCancellation::new(), |context| {
      Ok(count_processes(context.connection()))
    })
    .await
    .unwrap();
  assert_eq!(after, before + 1);
  database.close().await.unwrap();
}

#[tokio::test]
async fn a_failed_write_rolls_back_and_the_owner_stays_healthy() {
  let fixture = seeded_native().await;
  let database = fixture.open().await;
  let before = database
    .request_read(NativeCancellation::new(), |context| {
      Ok(count_processes(context.connection()))
    })
    .await
    .unwrap();

  let error = database
    .request_write(NativeCancellation::new(), |context| {
      context.with_transaction(|transaction| {
        transaction
          .connection()
          .execute(
            "INSERT INTO PROCESS_STATS VALUES (5000, 1, 'rolled-back', 1.0, 1, 1, '2026-09-03T00:00:00+00:00')",
            [],
          )
          .unwrap();
        Err::<(), _>(NativeDatabaseError::Worker {
          message: "deliberate failure after a write".to_owned(),
        })
      })
    })
    .await
    .unwrap_err();
  assert!(
    matches!(error, NativeDatabaseError::Worker { .. }),
    "{error:?}"
  );

  process_stats::insert(
    &database,
    NativeCancellation::new(),
    vec![process("after-rollback", 1.5)],
    "2026-09-03T00:00:00Z".parse().unwrap(),
  )
  .await
  .unwrap();
  let after = database
    .request_read(NativeCancellation::new(), |context| {
      Ok(count_processes(context.connection()))
    })
    .await
    .unwrap();
  assert_eq!(after, before + 1, "only the successful write survived");
  database.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bounded_request_capacity_makes_a_caller_wait() {
  let fixture = seeded_native().await;
  let database =
    hardviz_core::infrastructure::database::native_database::NativeDatabase::open(
      &fixture.finalized,
      NativeDatabaseOptions {
        expected_schema_version: app_native_schema::NATIVE_SCHEMA_VERSION,
        request_capacity: 1,
      },
    )
    .await
    .unwrap();

  let (started, mut started_rx) = tokio::sync::mpsc::channel::<()>(1);
  let (release, mut release_rx) = tokio::sync::mpsc::channel::<()>(1);
  let blocking = database.clone();
  let occupied = tokio::spawn(async move {
    blocking
      .request_read(NativeCancellation::new(), move |_| {
        started.blocking_send(()).unwrap();
        release_rx.blocking_recv().unwrap();
        Ok(())
      })
      .await
  });
  started_rx.recv().await.unwrap();

  // One request fits the bounded channel; the next caller has to wait for room
  // rather than queueing without limit.
  let queued = database.clone();
  let waiting = tokio::spawn(async move {
    queued
      .request_read(NativeCancellation::new(), |context| {
        Ok(count_processes(context.connection()))
      })
      .await
  });
  tokio::time::sleep(Duration::from_millis(200)).await;
  let refused = tokio::time::timeout(
    Duration::from_millis(300),
    database.request_read(NativeCancellation::new(), |context| {
      Ok(count_processes(context.connection()))
    }),
  )
  .await;
  assert!(refused.is_err(), "a third request must not be accepted yet");

  release.send(()).await.unwrap();
  occupied.await.unwrap().unwrap();
  waiting.await.unwrap().unwrap();
  database
    .request_read(NativeCancellation::new(), |context| {
      Ok(count_processes(context.connection()))
    })
    .await
    .unwrap();
  database.close().await.unwrap();
}

#[tokio::test]
async fn refuses_a_request_capacity_of_zero() {
  let fixture = seeded_native().await;
  let error =
    hardviz_core::infrastructure::database::native_database::NativeDatabase::open(
      &fixture.finalized,
      NativeDatabaseOptions {
        expected_schema_version: app_native_schema::NATIVE_SCHEMA_VERSION,
        request_capacity: 0,
      },
    )
    .await
    .unwrap_err();
  assert!(
    matches!(error, NativeDatabaseError::InvalidRequestCapacity),
    "{error:?}"
  );
}

async fn seeded_native() -> NativeFixture {
  let fixture = NativeFixture::new();
  let pool = fixture.migrated_pool().await;
  seed_domain_tables(&pool).await;
  pool.close().await;
  fixture.finalize().await;
  fixture
}

fn process(name: &str, cpu: f32) -> ProcessStatData {
  ProcessStatData {
    pid: 4242,
    process_name: name.to_owned(),
    cpu_usage: cpu,
    memory_usage: 4096,
    execution_sec: 60,
  }
}

fn count_processes(connection: &Connection) -> i64 {
  connection
    .query_row("SELECT COUNT(*) FROM PROCESS_STATS", [], |row| row.get(0))
    .unwrap()
}

fn storage_header_version(path: &Path) -> u64 {
  let bytes = std::fs::read(path).unwrap();
  // DuckDB reserves the first idx_t bytes for the block header, then writes
  // four magic bytes followed by the storage header version.
  u64::from_le_bytes(bytes[12..20].try_into().unwrap())
}

async fn highest_process_id(
  database: &hardviz_core::infrastructure::database::native_database::NativeDatabase,
) -> i64 {
  database
    .request_read(NativeCancellation::new(), |context| {
      Ok(
        context
          .connection()
          .query_row(
            "SELECT COALESCE(MAX(id), 0) FROM PROCESS_STATS",
            [],
            |row| row.get(0),
          )
          .unwrap(),
      )
    })
    .await
    .unwrap()
}

fn cpu_avg_union(
  connection: &Connection,
  id: i64,
) -> (Option<String>, Option<i64>, Option<f64>) {
  connection
    .query_row(
      "SELECT CAST(union_tag(cpu_avg) AS VARCHAR), union_extract(cpu_avg,'i'), \
       union_extract(cpu_avg,'r') FROM DATA_ARCHIVE WHERE id = ?",
      [id],
      |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .unwrap()
}

/// One row in every domain table, chosen so the copy has to preserve both
/// numeric tags, i64 extremes, embedded NUL text, NULLs, duplicated Process
/// identity tuples and every foreign key.
async fn seed_domain_tables(pool: &SqlitePool) {
  pool
    .execute(
      r#"
      INSERT INTO DATA_ARCHIVE
        (id,cpu_avg,cpu_max,cpu_min,ram_avg,ram_max,ram_min,timestamp,
         cpu_temperature_avg,cpu_temperature_max,cpu_power_avg,package_power_avg)
      VALUES
        (-9223372036854775808,-9223372036854775808,0,9223372036854775807,
         NULL,42,NULL,'2026-09-01T00:00:00+00:00',40.25,NULL,10.125,30.25),
        (-1,NULL,2,0,NULL,4,NULL,'2026-09-01T00:01:00+00:00',NULL,NULL,NULL,NULL),
        (0,NULL,40,10,NULL,60,NULL,'2026-09-01T00:02:00+00:00',
         45.12500000000001,50.0,-0.0,16.0),
        (1,0,0,0,NULL,0,NULL,'2026-09-01T00:03:00+00:00',NULL,NULL,NULL,NULL),
        (9223372036854775807,9223372036854775807,NULL,NULL,NULL,NULL,NULL,NULL,
         NULL,NULL,NULL,NULL);
      INSERT INTO GPU_DATA_ARCHIVE
        (id,gpu_name,usage_avg,usage_max,temperature_avg,temperature_max,
         timestamp,dedicated_memory_avg,gpu_id)
      VALUES
        (1,'Apple M4 Max',NULL,50,42,55,'2026-09-01T00:00:00+00:00',1024,'gpu-persisted-name'),
        (2,'Discrete GPU'||char(0)||'Secondary',NULL,NULL,NULL,60,NULL,NULL,NULL);
      INSERT INTO PROCESS_STATS(pid,process_name,cpu_usage,memory_usage,execution_sec,timestamp)
      VALUES
        (4000,'renderer',12.5,4096,60,'2026-09-01T00:00:00+00:00'),
        (4003,'legacy-wide',37.5,9223372036854775807,120,'2026-09-01T00:01:00+00:00'),
        (4000,'renderer',12.5,4096,180,'2026-09-01T00:02:00+00:00'),
        (4001,'helper'||char(0)||'worker',0.0,0,0,'2026-09-01T00:00:00+00:00'),
        (4000,'renderer',37.5,4096,120,'2026-09-01T00:03:00+00:00'),
        (4002,'idle',0.5,8192,2,'2026-09-01T00:04:00.500+00:00');
      INSERT INTO storage_devices
        (id,display_name,model,serial_hash,protocol,capacity_bytes,first_seen_at,last_seen_at,is_active)
      VALUES
        ('storage:hmac-sha256:v1:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
         'System SSD','NVMe Model','serial-hash','NVMe',9223372036854775807,'2026-01-01','2026-09-01',1);
      INSERT INTO storage_health_daily_records
        (device_id,date,health_status,temperature_celsius,power_on_hours,percentage_used,collected_at)
      VALUES
        ('storage:hmac-sha256:v1:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
         '2026-09-01','healthy',38.125,12345,2.5,'2026-09-01T23:59:00+00:00');
      INSERT INTO cooling_daily_summary
        (date,idle_cpu_temperature_avg,idle_sample_minutes,coverage_minutes,cpu_power_avg,power_sample_minutes)
      VALUES ('2026-09-01',35.25,240,720,22.25,600);
      INSERT INTO cooling_baseline VALUES
        (1,'2026-08-01','2026-08-30',36.25,7200,'2026-09-01T00:00:00+00:00');
      INSERT INTO cooling_hourly_summary VALUES
        ('2026-09-01T00:00:00+00:00',12.25,41.5,60);
      INSERT INTO AMBIENT_ARCHIVE(source,temperature,humidity,timestamp) VALUES
        ('SwitchBot Meter (a1b2)',24.125,45.5,'2026-09-01T00:00:00+00:00'),
        ('SwitchBot Meter (c3d4)',23.75,NULL,'2026-09-01T00:01:00+00:00');
      INSERT INTO FAN_ARCHIVE VALUES
        (0,'CPU Fan',0,'2026-09-01T00:00:00+00:00'),
        (-1,'System Fan',1400,'2026-09-01T00:01:00+00:00');
      INSERT INTO cooling_fan_daily_summary VALUES
        ('2026-09-01','CPU Fan',1250.25,2200,0,720);
      INSERT INTO cooling_delta_baseline VALUES
        (1,'SwitchBot Meter (a1b2)','2026-08-01','2026-08-30',12.125,7200,
         '2026-09-01T00:00:00+00:00');
      INSERT INTO cooling_thermal_delta_daily_summary
        (date,source,coverage_minutes,idle_delta_temperature_avg,idle_delta_sample_minutes)
      VALUES ('2026-09-01','SwitchBot Meter (a1b2)',700,10.0,200);
      INSERT INTO cooling_covariate_daily_summary
        (date,source,band,sample_minutes,band_share,ambient_temperature_median,
         delta_minutes,delta_temperature_median,power_minutes,cpu_power_median,
         power_fit_n,power_fit_sum_x,power_fit_sum_y,power_fit_sum_xy,power_fit_sum_xx,power_fit_sum_yy)
      VALUES ('2026-09-01','SwitchBot Meter (a1b2)','low',300,0.25,24.125,
         300,20.5,280,22.25,280,6230.0,5740.0,127755.0,140000.0,120000.0);
      INSERT INTO cooling_fan_covariate_daily_summary VALUES
        ('2026-09-01','SwitchBot Meter (a1b2)','CPU Fan','low',280,1250.25,
         280,350000.0,5740.0,7175000.0,438000000.0,120000.0);
      "#,
    )
    .await
    .unwrap();

  // The same `Option<f32>` binding the production Hardware and GPU writers use:
  // an INTEGER-affinity column stores a fractional reading as a REAL cell.
  sqlx::query("UPDATE DATA_ARCHIVE SET cpu_avg = ? WHERE id = 0")
    .bind(Some(25.25_f32))
    .execute(pool)
    .await
    .unwrap();
  let stored_device: String =
    sqlx::query_scalar("SELECT device_id FROM storage_health_daily_records")
      .fetch_one(pool)
      .await
      .unwrap();
  assert_eq!(stored_device, STORAGE_ID);
}
