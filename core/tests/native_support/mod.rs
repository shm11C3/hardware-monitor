//! Shared fixture for the native DuckDB backend tests.
//!
//! Everything goes through the real path: App's ordered SQLite migrations,
//! Core's migrator, the #2088 candidate builder, and finalization against App's
//! own stable schema definition. Nothing here hand-writes a native file, so a
//! test can only pass if the production conversion produced it.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use hardviz_core::infrastructure::database::candidate_database::{
  CandidateError, CandidateReport, build_candidate_database,
};
use hardviz_core::infrastructure::database::migrate;
use hardviz_core::infrastructure::database::native_database::{
  AUTHORITY_MARKER_FILE_NAME, AuthorityPaths, AuthorityState, NativeDatabase,
  NativeDatabaseError, NativeDatabaseOptions, NativeFinalizationReport,
  NativeReconciliationReport, finalize_candidate_database, inspect_authority,
  observe_authority, reconcile_native_database,
};
use sha2::{Digest, Sha256};
use sqlx::ConnectOptions;
use sqlx::sqlite::{
  SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions,
};
use tempfile::TempDir;

#[path = "../../../src-tauri/src/infrastructure/database/migration.rs"]
pub mod app_migrations;

#[path = "../../../src-tauri/src/infrastructure/database/native_schema.rs"]
pub mod app_native_schema;

/// A SQLite source, the candidate copied from it, and the finalized native
/// database, all inside one temporary directory.
pub struct NativeFixture {
  pub directory: TempDir,
  pub source: PathBuf,
  pub candidate: PathBuf,
  pub finalized: PathBuf,
}

impl NativeFixture {
  pub fn new() -> Self {
    let directory = tempfile::tempdir().unwrap();
    Self {
      source: directory.path().join("source.sqlite3"),
      candidate: directory.path().join("candidate.duckdb"),
      finalized: directory.path().join("finalized.duckdb"),
      directory,
    }
  }

  pub async fn migrated_pool(&self) -> SqlitePool {
    let pool = open_pool(&self.source, true).await;
    migrate::run_on_pool(&pool, app_migrations::get_migrations())
      .await
      .unwrap();
    pool
  }

  pub async fn build_candidate(&self) -> Result<CandidateReport, CandidateError> {
    build_candidate_database(
      &self.source,
      &self.candidate,
      app_migrations::get_migrations(),
    )
    .await
  }

  pub async fn try_finalize(
    &self,
  ) -> Result<NativeFinalizationReport, NativeDatabaseError> {
    finalize_candidate_database(
      &self.candidate,
      &self.finalized,
      app_native_schema::get_native_schema(),
    )
    .await
  }

  /// Copy the source into a candidate and finalize it into the stable schema.
  pub async fn finalize(&self) -> NativeFinalizationReport {
    self.build_candidate().await.unwrap();
    self.try_finalize().await.unwrap()
  }

  /// Finalize into caller-chosen paths, so a test can build a second, freshly
  /// converted database beside the one it reconciled.
  pub async fn finalize_into(
    &self,
    candidate: &Path,
    finalized: &Path,
  ) -> NativeFinalizationReport {
    build_candidate_database(&self.source, candidate, app_migrations::get_migrations())
      .await
      .unwrap();
    finalize_candidate_database(
      candidate,
      finalized,
      app_native_schema::get_native_schema(),
    )
    .await
    .unwrap()
  }

  pub async fn try_reconcile(
    &self,
  ) -> Result<NativeReconciliationReport, NativeDatabaseError> {
    reconcile_native_database(
      &self.source,
      &self.finalized,
      app_migrations::get_migrations(),
      app_native_schema::get_native_schema(),
    )
    .await
  }

  pub fn authority_paths(&self) -> AuthorityPaths {
    AuthorityPaths {
      source_database: self.source.clone(),
      native_database: self.finalized.clone(),
      marker: self.directory.path().join(AUTHORITY_MARKER_FILE_NAME),
    }
  }

  pub fn authority_state(&self) -> AuthorityState {
    inspect_authority(&observe_authority(
      &self.authority_paths(),
      app_native_schema::NATIVE_SCHEMA_VERSION,
    ))
  }

  pub async fn try_open(
    &self,
    expected_version: u32,
  ) -> Result<NativeDatabase, NativeDatabaseError> {
    NativeDatabase::open(
      &self.finalized,
      NativeDatabaseOptions::new(expected_version),
    )
    .await
  }

  pub async fn open(&self) -> NativeDatabase {
    self
      .try_open(app_native_schema::NATIVE_SCHEMA_VERSION)
      .await
      .unwrap()
  }

  pub fn work_directories(&self) -> Vec<String> {
    std::fs::read_dir(self.directory.path())
      .unwrap()
      .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
      .filter(|name| name.starts_with(".hardwarevisualizer-duckdb-"))
      .collect()
  }
}

pub async fn open_pool(path: &Path, create: bool) -> SqlitePool {
  let options = SqliteConnectOptions::new()
    .filename(path)
    .create_if_missing(create)
    .foreign_keys(true)
    .journal_mode(SqliteJournalMode::Wal)
    .disable_statement_logging();
  SqlitePoolOptions::new()
    .max_connections(1)
    .connect_with(options)
    .await
    .unwrap()
}

pub fn file_hash(path: &Path) -> String {
  format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}

pub fn read_only(path: &Path) -> duckdb::Connection {
  let config = duckdb::Config::default()
    .access_mode(duckdb::AccessMode::ReadOnly)
    .unwrap();
  duckdb::Connection::open_with_flags(path, config).unwrap()
}

/// The production epoch-millisecond adapter,
/// `archive_queries::sqlite_epoch_milliseconds_of`, restated so a test can run
/// it against SQLite independently of the finalizer. It is `pub(crate)` in Core,
/// so this copy is the pinned specification: a change there must be mirrored
/// here deliberately.
pub fn sqlite_epoch_milliseconds_of(column: &str) -> String {
  format!(
    "(CAST(strftime('%s', {column}) AS INTEGER) * 1000 + \
     CAST(substr(strftime('%f', {column}), 4, 3) AS INTEGER))"
  )
}
