#![cfg(feature = "duckdb-archive")]
//! Reconciliation of a finalized native database against its live SQLite
//! source, and the durable authority selection that may follow it.
//!
//! Every database here is produced by the production path - App's migrations,
//! the #2088 candidate builder, finalization, reconciliation - so the oracle
//! for "reconciliation worked" is a second database converted from scratch from
//! the same source: the two must hold the same rows.

mod native_support;

use hardviz_core::infrastructure::database::migrate::{self, SchemaMigration};
use hardviz_core::infrastructure::database::native_database::{
  AuthorityInconsistency, AuthorityRecovery, AuthorityState, NativeDatabaseError,
  VerifiedNativeDatabase, inspect_authority, observe_authority,
  reconcile_native_database, repair_authority_marker, select_native_database,
};
use native_support::{
  NativeFixture, app_migrations, app_native_schema, file_hash, open_pool, read_only,
};
use sqlx::{Executor, SqlitePool};

/// The archive tables change in all three ways between a snapshot and the
/// moment a conversion finishes: new rows arrive, a summary is recomputed in
/// place, and Retention Period pruning removes old rows. The reconciled
/// database must end up holding exactly what a conversion started now would.
#[tokio::test]
async fn reconciliation_applies_inserts_updates_and_deletes_and_matches_a_fresh_conversion()
 {
  let fixture = NativeFixture::new();
  let pool = fixture.migrated_pool().await;
  seed(&pool).await;
  pool.close().await;
  fixture.finalize().await;
  let before = table_rows(&fixture, "DATA_ARCHIVE");

  let pool = open_pool(&fixture.source, false).await;
  pool
    .execute(
      r#"
      -- New readings, the way the collector appends them.
      INSERT INTO DATA_ARCHIVE(id,cpu_avg,ram_max,timestamp,cpu_temperature_avg)
        VALUES (100,55,70,'2026-09-02T00:00:00+00:00',41.5),
               (101,56,71,'2026-09-02T00:01:00+00:00',41.75);
      -- Retention pruning, which the append-only sketch could not express.
      DELETE FROM DATA_ARCHIVE WHERE id = -1;
      -- A daily summary recomputed in place, under an unchanged primary key.
      UPDATE cooling_daily_summary
        SET coverage_minutes = 1440, idle_cpu_temperature_avg = 34.5
        WHERE date = '2026-09-01';
      -- A composite-key row updated and another deleted.
      UPDATE cooling_covariate_daily_summary SET sample_minutes = 600
        WHERE date = '2026-09-01' AND source = 'ambient' AND band = 'low';
      DELETE FROM cooling_fan_covariate_daily_summary WHERE band = 'low';
      INSERT INTO AMBIENT_ARCHIVE(source,temperature,humidity,timestamp)
        VALUES ('ambient',22.5,40.0,'2026-09-02T00:00:00+00:00');
      -- Process rows are pruned wholesale; the allocator must not reuse ids.
      INSERT INTO PROCESS_STATS(pid,process_name,cpu_usage,memory_usage,execution_sec,timestamp)
        VALUES (5000,'new-worker',3.5,2048,10,'2026-09-02T00:00:00+00:00');
      DELETE FROM PROCESS_STATS WHERE pid = 4000;
      "#,
    )
    .await
    .unwrap();
  pool.close().await;
  let source_hash = file_hash(&fixture.source);

  let report = fixture.try_reconcile().await.unwrap();

  // The source was only read, and nothing was left beside the database.
  assert_eq!(file_hash(&fixture.source), source_hash);
  assert!(fixture.work_directories().is_empty());
  assert_eq!(report.state, "finalized_unselected");
  assert_eq!(report.tables.len(), 15);
  // Appending integer readings changes no storage class, so the provenance the
  // file records is unchanged.
  assert_eq!(
    report.previous_source_schema_sha256,
    report.source_schema_sha256
  );

  let archive = table(&report, "DATA_ARCHIVE");
  assert_eq!(archive.inserted_rows, 2);
  assert_eq!(archive.deleted_rows, 1);
  assert_eq!(archive.updated_rows, 0);
  assert_eq!(archive.candidate_rows, before + 2 - 1);
  let cooling = table(&report, "cooling_daily_summary");
  assert_eq!(
    (
      cooling.updated_rows,
      cooling.inserted_rows,
      cooling.deleted_rows
    ),
    (1, 0, 0)
  );
  let covariate = table(&report, "cooling_covariate_daily_summary");
  assert_eq!(covariate.updated_rows, 1);
  let fan_covariate = table(&report, "cooling_fan_covariate_daily_summary");
  assert_eq!(fan_covariate.deleted_rows, 1);
  assert_eq!(fan_covariate.candidate_rows, 0);
  let process = table(&report, "PROCESS_STATS");
  assert_eq!((process.inserted_rows, process.deleted_rows), (1, 3));
  // A table nothing touched is recognized as unchanged rather than rewritten.
  let devices = table(&report, "storage_devices");
  assert_eq!(devices.unchanged_rows, 1);
  assert_eq!(
    (
      devices.inserted_rows,
      devices.updated_rows,
      devices.deleted_rows
    ),
    (0, 0, 0)
  );
  assert_eq!(
    report.inserted_rows,
    report
      .tables
      .iter()
      .map(|table| table.inserted_rows)
      .sum::<u64>()
  );

  // The oracle: a second database converted from scratch from the same source.
  let fresh_candidate = fixture.directory.path().join("fresh-candidate.duckdb");
  let fresh_finalized = fixture.directory.path().join("fresh.duckdb");
  let fresh = fixture
    .finalize_into(&fresh_candidate, &fresh_finalized)
    .await;
  assert_eq!(report.total_rows, fresh.total_rows);
  for reconciled in &report.tables {
    let expected = fresh
      .tables
      .iter()
      .find(|table| table.name == reconciled.name)
      .unwrap();
    assert_eq!(
      (
        reconciled.reopened_rows,
        reconciled.reopened_digest.as_str()
      ),
      (expected.copied_rows, expected.copied_digest.as_str()),
      "{}",
      reconciled.name
    );
    assert_eq!(reconciled.expected_digest, reconciled.reopened_digest);
  }

  // The re-imported allocator state matches the fresh conversion's, so a
  // pruned AUTOINCREMENT table cannot hand out an id SQLite already used.
  assert_eq!(identities(&fixture.finalized), identities(&fresh_finalized));
  let high_water = identities(&fixture.finalized)
    .into_iter()
    .find(|(table, ..)| table == "PROCESS_STATS")
    .unwrap()
    .2;
  assert_eq!(high_water, 7);

  // The reconciled file is still an ordinary native database to its owner.
  let database = fixture.open().await;
  database.close().await.unwrap();
}

/// The whole reconciliation is one transaction. The proof is a production
/// failure mode rather than a test hook: a legacy REAL in an INTEGER-declared
/// column is refused by the third table, after two tables have already been
/// written, and the committed file must show neither.
#[tokio::test]
async fn a_refused_cell_leaves_the_earlier_tables_untouched() {
  let fixture = NativeFixture::new();
  let pool = fixture.migrated_pool().await;
  seed(&pool).await;
  pool.close().await;
  fixture.finalize().await;
  let before = file_hash(&fixture.finalized);
  let archive_before = table_rows(&fixture, "DATA_ARCHIVE");
  let gpu_before = table_rows(&fixture, "GPU_DATA_ARCHIVE");

  let pool = open_pool(&fixture.source, false).await;
  // Rows the first two tables would have taken.
  pool
    .execute(
      r#"
      INSERT INTO DATA_ARCHIVE(id,cpu_avg,timestamp) VALUES (200,7,'2026-09-02T00:00:00+00:00');
      INSERT INTO GPU_DATA_ARCHIVE(id,gpu_name,timestamp) VALUES (200,'Late GPU','2026-09-02T00:00:00+00:00');
      "#,
    )
    .await
    .unwrap();
  // `memory_usage` is `BIGINT NOT NULL` in the stable schema. A fractional
  // value in the INTEGER-affinity SQLite column stays a REAL cell.
  sqlx::query("UPDATE PROCESS_STATS SET memory_usage = ? WHERE pid = 4002")
    .bind(Some(2048.5_f64))
    .execute(&pool)
    .await
    .unwrap();
  pool.close().await;

  let error = fixture.try_reconcile().await.unwrap_err();

  // The recorded source digest also covers observed storage classes, so this
  // edit changes it. Reconciliation must still read the row and refuse the
  // cell, not mistake a legacy value for a migration.
  match error {
    NativeDatabaseError::UnrepresentableCell {
      ref table,
      ref column,
      candidate,
      ..
    } => {
      assert_eq!(table, "PROCESS_STATS");
      assert_eq!(column, "memory_usage");
      assert_eq!(candidate, "a real");
    }
    other => panic!("unexpected error: {other:?}"),
  }
  // Neither of the two tables the transaction had already written survives,
  // and the file is byte-identical to the one reconciliation started from.
  assert_eq!(table_rows(&fixture, "DATA_ARCHIVE"), archive_before);
  assert_eq!(table_rows(&fixture, "GPU_DATA_ARCHIVE"), gpu_before);
  assert_eq!(file_hash(&fixture.finalized), before);
  assert!(fixture.work_directories().is_empty());
}

/// A migration that ran after the file was finalized changes the shape of the
/// rows. The structural comparison against the App-owned stable schema is what
/// catches it, and it runs for every table before the transaction opens.
#[tokio::test]
async fn a_source_schema_that_moved_on_is_refused_before_any_write() {
  let fixture = NativeFixture::new();
  let pool = fixture.migrated_pool().await;
  seed(&pool).await;
  pool.close().await;
  fixture.finalize().await;
  let before = file_hash(&fixture.finalized);

  let mut migrations = app_migrations::get_migrations();
  let next = migrations
    .iter()
    .map(|migration| migration.version)
    .max()
    .unwrap()
    + 1;
  migrations.push(SchemaMigration {
    version: next,
    description: "add a column after finalization",
    sql: "ALTER TABLE FAN_ARCHIVE ADD COLUMN note TEXT;",
  });
  let pool = open_pool(&fixture.source, false).await;
  migrate::run_on_pool(&pool, migrations.clone())
    .await
    .unwrap();
  pool.close().await;

  let error = reconcile_native_database(
    &fixture.source,
    &fixture.finalized,
    migrations,
    app_native_schema::get_native_schema(),
  )
  .await
  .unwrap_err();

  match error {
    NativeDatabaseError::SchemaMismatch {
      ref table,
      ref detail,
    } => {
      assert_eq!(table, "FAN_ARCHIVE");
      assert!(detail.contains("note"), "{detail}");
    }
    other => panic!("unexpected error: {other:?}"),
  }
  assert_eq!(file_hash(&fixture.finalized), before);
  assert!(fixture.work_directories().is_empty());
}

/// Selection is the one irreversible step, so the database records it before
/// the marker does and both have to agree afterwards.
#[tokio::test]
async fn selecting_records_the_database_first_and_then_the_marker() {
  let fixture = NativeFixture::new();
  let pool = fixture.migrated_pool().await;
  seed(&pool).await;
  pool.close().await;
  fixture.finalize().await;
  assert_eq!(
    fixture.authority_state(),
    AuthorityState::FinalizedUnselected
  );

  let report = fixture.try_reconcile().await.unwrap();
  let paths = fixture.authority_paths();
  let marker = select_native_database(
    paths.clone(),
    VerifiedNativeDatabase::from_reconciliation(&report),
  )
  .await
  .unwrap();

  assert_eq!(marker.native_database_file_name, "finalized.duckdb");
  assert_eq!(marker.source_schema_sha256, report.source_schema_sha256);
  assert_eq!(marker.total_rows, report.total_rows);
  assert!(paths.marker.is_file());
  assert_eq!(fixture.authority_state(), AuthorityState::NativeSelected);
  assert_eq!(state_of(&fixture), "selected");

  // A selected database is the same finalized file, so the owner still serves
  // it. Refusing here would break the backend exactly once it became
  // authoritative.
  fixture.open().await.close().await.unwrap();

  // Selecting again is a caller mistake, not an idempotent no-op.
  let error =
    select_native_database(paths, VerifiedNativeDatabase::from_reconciliation(&report))
      .await
      .unwrap_err();
  assert!(matches!(error, NativeDatabaseError::UnexpectedState { .. }));
}

/// The only crash window the write order leaves: the selection committed and
/// the marker never landed. It is repaired from the database, never guessed.
#[tokio::test]
async fn a_marker_lost_after_the_commit_is_repaired_from_the_database() {
  let fixture = NativeFixture::new();
  let pool = fixture.migrated_pool().await;
  seed(&pool).await;
  pool.close().await;
  let finalization = fixture.finalize().await;
  let paths = fixture.authority_paths();
  select_native_database(
    paths.clone(),
    VerifiedNativeDatabase::from_finalization(&finalization),
  )
  .await
  .unwrap();

  std::fs::remove_file(&paths.marker).unwrap();
  assert_eq!(
    fixture.authority_state(),
    AuthorityState::Inconsistent {
      reason: AuthorityInconsistency::SelectedWithoutMarker,
      recovery: AuthorityRecovery::RepairSelectionMarkerFromNativeMetadata,
    }
  );

  let repaired = repair_authority_marker(&paths).unwrap();
  assert_eq!(
    repaired.source_schema_sha256,
    finalization.source_schema_sha256
  );
  assert_eq!(fixture.authority_state(), AuthorityState::NativeSelected);

  // A marker that names a database this directory does not hold is reported,
  // not repaired: the numbers needed to decide belong to the maintainer.
  let elsewhere =
    hardviz_core::infrastructure::database::native_database::AuthorityPaths {
      native_database: fixture.directory.path().join("other.duckdb"),
      ..paths
    };
  let facts = observe_authority(&elsewhere, app_native_schema::NATIVE_SCHEMA_VERSION);
  assert_eq!(
    inspect_authority(&facts),
    AuthorityState::Inconsistent {
      reason: AuthorityInconsistency::MarkerWithoutNativeDatabase,
      recovery: AuthorityRecovery::StopAndReport,
    }
  );
}

/// A verified conversion is the only thing that may be selected, so a file that
/// moved on since it was verified is refused rather than published.
#[tokio::test]
async fn a_database_that_is_not_the_verified_conversion_cannot_be_selected() {
  let fixture = NativeFixture::new();
  let pool = fixture.migrated_pool().await;
  seed(&pool).await;
  pool.close().await;
  let finalization = fixture.finalize().await;

  let pool = open_pool(&fixture.source, false).await;
  pool
    .execute(
      "INSERT INTO DATA_ARCHIVE(id,cpu_avg,timestamp) \
       VALUES (300,9,'2026-09-02T00:00:00+00:00');",
    )
    .await
    .unwrap();
  pool.close().await;
  fixture.try_reconcile().await.unwrap();

  // The finalization report describes the file as it was before reconciliation.
  let error = select_native_database(
    fixture.authority_paths(),
    VerifiedNativeDatabase::from_finalization(&finalization),
  )
  .await
  .unwrap_err();
  assert!(matches!(
    error,
    NativeDatabaseError::UnverifiedSelection { .. }
  ));
  assert!(!fixture.authority_paths().marker.exists());
  assert_eq!(state_of(&fixture), "finalized_unselected");
}

/// Once the native database is authoritative there is nothing to reconcile
/// into it: SQLite is no longer the side that moved on.
#[tokio::test]
async fn a_selected_database_is_not_reconciled_again() {
  let fixture = NativeFixture::new();
  let pool = fixture.migrated_pool().await;
  seed(&pool).await;
  pool.close().await;
  let finalization = fixture.finalize().await;
  select_native_database(
    fixture.authority_paths(),
    VerifiedNativeDatabase::from_finalization(&finalization),
  )
  .await
  .unwrap();

  let error = fixture.try_reconcile().await.unwrap_err();

  match error {
    NativeDatabaseError::UnexpectedState {
      operation,
      ref state,
      expected,
    } => {
      assert_eq!(operation, "reconciled");
      assert_eq!(state, "selected");
      assert_eq!(expected, "finalized_unselected");
    }
    other => panic!("unexpected error: {other:?}"),
  }
  assert!(fixture.work_directories().is_empty());
}

fn table<'a>(
  report: &'a hardviz_core::infrastructure::database::native_database::NativeReconciliationReport,
  name: &str,
) -> &'a hardviz_core::infrastructure::database::native_database::NativeReconciliationTableReport
{
  report
    .tables
    .iter()
    .find(|table| table.name == name)
    .unwrap()
}

fn table_rows(fixture: &NativeFixture, table: &str) -> u64 {
  let connection = read_only(&fixture.finalized);
  let count: i64 = connection
    .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
      row.get(0)
    })
    .unwrap();
  u64::try_from(count).unwrap()
}

fn state_of(fixture: &NativeFixture) -> String {
  read_only(&fixture.finalized)
    .query_row("SELECT state FROM __hv_native_metadata", [], |row| {
      row.get(0)
    })
    .unwrap()
}

fn identities(path: &std::path::Path) -> Vec<(String, String, i64)> {
  let connection = read_only(path);
  let mut statement = connection
    .prepare(
      "SELECT table_name, mode, high_water FROM __hv_native_identities \
       ORDER BY table_name",
    )
    .unwrap();
  statement
    .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
    .unwrap()
    .collect::<Result<Vec<_>, _>>()
    .unwrap()
}

/// One row in every table the stable schema declares, including the composite
/// keyed cooling summaries, so the merge is exercised over every key shape.
async fn seed(pool: &SqlitePool) {
  pool
    .execute(
      r#"
      INSERT INTO DATA_ARCHIVE(id,cpu_avg,cpu_max,ram_max,timestamp,cpu_temperature_avg)
      VALUES (-1,10,20,30,'2026-09-01T00:00:00+00:00',40.25),
             (1,11,21,31,'2026-09-01T00:01:00+00:00',40.5),
             (2,12,22,32,'2026-09-01T00:02:00+00:00',NULL);
      INSERT INTO GPU_DATA_ARCHIVE(id,gpu_name,usage_avg,temperature_max,timestamp,gpu_id)
      VALUES (1,'Apple M4 Max',50,55,'2026-09-01T00:00:00+00:00','gpu-a');
      INSERT INTO PROCESS_STATS(pid,process_name,cpu_usage,memory_usage,execution_sec,timestamp)
      VALUES (4000,'renderer',12.5,4096,60,'2026-09-01T00:00:00+00:00'),
             (4000,'renderer',12.5,4096,120,'2026-09-01T00:01:00+00:00'),
             (4000,'renderer',12.5,4096,180,'2026-09-01T00:02:00+00:00'),
             (4002,'idle',0.5,8192,2,'2026-09-01T00:03:00+00:00'),
             (4003,'helper',1.5,1024,4,'2026-09-01T00:04:00+00:00'),
             (4004,'other',2.5,512,8,'2026-09-01T00:05:00+00:00');
      INSERT INTO storage_devices
        (id,display_name,model,serial_hash,protocol,capacity_bytes,first_seen_at,last_seen_at,is_active)
      VALUES ('storage:hmac-sha256:v1:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
              'System SSD','NVMe Model','serial-hash','NVMe',1024,'2026-01-01','2026-09-01',1);
      INSERT INTO storage_health_daily_records
        (device_id,date,health_status,temperature_celsius,power_on_hours,percentage_used,collected_at)
      VALUES ('storage:hmac-sha256:v1:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
              '2026-09-01','healthy',38.125,12345,2.5,'2026-09-01T23:59:00+00:00');
      INSERT INTO cooling_daily_summary
        (date,idle_cpu_temperature_avg,idle_sample_minutes,coverage_minutes,cpu_power_avg,power_sample_minutes)
      VALUES ('2026-09-01',35.25,240,720,22.25,600);
      INSERT INTO cooling_baseline VALUES
        (1,'2026-08-01','2026-08-30',36.25,7200,'2026-09-01T00:00:00+00:00');
      INSERT INTO cooling_hourly_summary VALUES
        ('2026-09-01T00:00:00+00:00',12.25,41.5,60);
      INSERT INTO AMBIENT_ARCHIVE(source,temperature,humidity,timestamp)
      VALUES ('ambient',24.125,45.5,'2026-09-01T00:00:00+00:00');
      INSERT INTO FAN_ARCHIVE VALUES (1,'CPU Fan',1200,'2026-09-01T00:00:00+00:00');
      INSERT INTO cooling_fan_daily_summary VALUES
        ('2026-09-01','CPU Fan',1250.25,2200,0,720);
      INSERT INTO cooling_delta_baseline VALUES
        (1,'ambient','2026-08-01','2026-08-30',12.125,7200,'2026-09-01T00:00:00+00:00');
      INSERT INTO cooling_thermal_delta_daily_summary
        (date,source,coverage_minutes,idle_delta_temperature_avg,idle_delta_sample_minutes)
      VALUES ('2026-09-01','ambient',700,10.0,200);
      INSERT INTO cooling_covariate_daily_summary
        (date,source,band,sample_minutes,band_share,ambient_temperature_median,
         delta_minutes,delta_temperature_median,power_minutes,cpu_power_median,
         power_fit_n,power_fit_sum_x,power_fit_sum_y,power_fit_sum_xy,power_fit_sum_xx,power_fit_sum_yy)
      VALUES ('2026-09-01','ambient','low',300,0.25,24.125,300,20.5,280,22.25,
              280,6230.0,5740.0,127755.0,140000.0,120000.0);
      INSERT INTO cooling_fan_covariate_daily_summary VALUES
        ('2026-09-01','ambient','CPU Fan','low',280,1250.25,280,350000.0,5740.0,
         7175000.0,438000000.0,120000.0);
      "#,
    )
    .await
    .unwrap();
}
