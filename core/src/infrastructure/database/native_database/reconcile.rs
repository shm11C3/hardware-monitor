//! Brings a finalized native database back up to date with its SQLite source.
//!
//! Finalization ([`super::finalize`]) copies one pinned snapshot. The
//! application keeps writing to SQLite while that copy runs, so the finalized
//! file is stale the moment it exists. Reconciliation closes that gap: it
//! captures a *new* #2088 candidate from the live source and makes the
//! finalized file equal to it, so the post-condition is the same one
//! finalization already guarantees - the native database holds exactly the
//! finalized representation of a validated snapshot.
//!
//! # Why a second candidate rather than a direct SQLite read
//!
//! The candidate builder already owns the parts that are hard to get right: a
//! pinned read transaction, canonical-cell validation, per-row size bounds, and
//! the recorded source-schema digest. Reading SQLite a second way here would
//! duplicate all of it and let the two readers drift. The cost is one extra
//! file on disk for the duration, which [`super::preflight`] budgets for.
//!
//! # Why every table is diffed by primary key
//!
//! An earlier sketch treated the archive tables as append-only and carried
//! forward only the rows above a stored id high-water mark. That is not true of
//! this schema: Retention Period pruning deletes from `DATA_ARCHIVE`,
//! `PROCESS_STATS`, `AMBIENT_ARCHIVE` and `FAN_ARCHIVE`, and the cooling and
//! Storage Health summaries are recomputed in place. A high-water path would
//! therefore need a second declaration of which tables may lose rows, kept in
//! step with the retention code by hand - and it would save nothing, because
//! the mandated reopen verification reads every row back either way. One
//! uniform merge over the native primary key is both smaller and correct for
//! inserts, updates and deletes alike.
//!
//! # Why the recorded source-schema digest is not the drift gate
//!
//! The candidate records a `source_schema_sha256` that looks like a schema
//! digest but is not one: the candidate builder recomputes it after scanning
//! the rows, so it also covers the storage classes each column was *observed*
//! to hold. A production writer binding its first `Option<f32>` into an
//! INTEGER-declared column changes that digest without any migration having
//! run, and refusing reconciliation there would strand the conversion on
//! exactly the data shape the tagged-union columns exist to carry.
//!
//! The gate that matters is structural and already exists.
//! [`require_every_candidate_table_is_declared`] compares the table sets and
//! [`plan_columns`] compares the column names in both directions, for every
//! table, before the transaction opens - so a migration that added, removed or
//! renamed anything is refused with the table and column named, and nothing is
//! written. Both digests are carried in the report so a caller can still see
//! that the source moved on.
//!
//! # Atomicity
//!
//! All fifteen tables, the re-imported identity high-water marks and the
//! metadata row are written inside a single DuckDB transaction. Staging tables
//! are created before it begins and dropped after it commits, so an
//! interruption anywhere - including a cell the stable column cannot hold,
//! discovered on the last table - leaves the pre-reconciliation database
//! exactly as it was.

use std::path::{Path, PathBuf};

use duckdb::types::Value;
use duckdb::{AccessMode, Connection};

use super::NativeDatabaseError;
use super::cell::{
  COPY_BATCH_ROWS, Cell, NativeColumnKind, RowMultisetDigest, quote_identifier,
};
use super::compatibility::verify_storage_version;
use super::epoch::EpochMilliseconds;
use super::finalize::{
  ColumnPlan, ColumnSource, FINALIZED_UNSELECTED, NATIVE_IDENTITY_TABLE,
  NATIVE_METADATA_TABLE, SOURCE_ORDINAL_COLUMN, TableColumn, append_staging, count_rows,
  create_staging_sql, derive_epoch_milliseconds, insert_from_staging_sql, open_database,
  plan_columns, read_candidate_provenance, read_columns, read_primary_key,
  require_every_candidate_table_is_declared, require_no_wal, stage_cell,
  write_identities,
};
use super::paging::{PagedReader, ReadColumn};
use super::schema::NativeSchemaDefinition;
use super::selection::VerifiedNativeDatabase;
use crate::infrastructure::database::candidate_database::build_candidate_database;
use crate::infrastructure::database::migrate::SchemaMigration;

const WORK_PREFIX: &str = ".hardwarevisualizer-duckdb-reconcile-";

/// What one table's reconciliation changed, and what it reads back as.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeReconciliationTableReport {
  pub name: String,
  /// Rows the freshly captured candidate holds, which is what the table must
  /// end up holding.
  pub candidate_rows: u64,
  pub inserted_rows: u64,
  pub updated_rows: u64,
  pub deleted_rows: u64,
  pub unchanged_rows: u64,
  pub reopened_rows: u64,
  /// Order-independent digest of the candidate in its finalized
  /// representation: what the table is required to equal.
  pub expected_digest: String,
  /// The same digest recomputed after the database was closed and reopened.
  pub reopened_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeReconciliationReport {
  pub native_database_path: PathBuf,
  pub state: String,
  pub schema_version: u32,
  /// The new candidate's recorded source digest, which the reconciled file now
  /// records too.
  pub source_schema_sha256: String,
  /// What the file recorded before. It differs whenever the source's observed
  /// storage classes moved, which is not by itself a schema change - see the
  /// module documentation.
  pub previous_source_schema_sha256: String,
  pub tables: Vec<NativeReconciliationTableReport>,
  pub total_rows: u64,
  pub inserted_rows: u64,
  pub updated_rows: u64,
  pub deleted_rows: u64,
  pub native_bytes: u64,
}

/// Make `native_database` equal a newly captured snapshot of `source`.
///
/// The SQLite source is only read, under the candidate builder's pinned
/// transaction. The native database is left untouched unless the whole
/// reconciliation commits, and the committed result is verified after the file
/// has been closed and reopened.
///
/// The [`VerifiedNativeDatabase`] returned beside the report is the only proof
/// [`super::select_native_database`] accepts, so a backend can be made
/// authoritative only from a file this call has just caught up and read back.
///
/// Opens the native database as a DuckDB instance of its own, so no
/// [`super::NativeDatabase`] owner may be live on the same file while it runs.
pub async fn reconcile_native_database(
  source: &Path,
  native_database: &Path,
  migrations: Vec<SchemaMigration>,
  schema: NativeSchemaDefinition,
) -> Result<(NativeReconciliationReport, VerifiedNativeDatabase), NativeDatabaseError> {
  let metadata =
    std::fs::metadata(native_database).map_err(|_| NativeDatabaseError::Unavailable {
      path: native_database.to_owned(),
    })?;
  if !metadata.is_file() {
    return Err(NativeDatabaseError::Unavailable {
      path: native_database.to_owned(),
    });
  }
  let parent = native_database
    .parent()
    .filter(|path| !path.as_os_str().is_empty())
    .ok_or_else(|| NativeDatabaseError::SourceSnapshot {
      message: "the native database must have a parent directory".to_owned(),
    })?;

  // Declared before every connection so the connections drop first: Windows
  // cannot remove an open database file.
  let work = tempfile::Builder::new()
    .prefix(WORK_PREFIX)
    .tempdir_in(parent)
    .map_err(|error| NativeDatabaseError::SourceSnapshot {
      message: format!("failed to reserve a reconciliation work directory: {error}"),
    })?;
  let candidate_path = work.path().join("candidate.duckdb");
  build_candidate_database(source, &candidate_path, migrations)
    .await
    .map_err(|error| NativeDatabaseError::SourceSnapshot {
      message: error.to_string(),
    })?;

  let native_database = native_database.to_owned();
  let report = tokio::task::spawn_blocking(move || {
    let result = reconcile(&candidate_path, &native_database, schema, work.path());
    drop(work);
    result
  })
  .await
  .map_err(|error| NativeDatabaseError::Worker {
    message: error.to_string(),
  })??;
  let verified = VerifiedNativeDatabase::from_reconciliation(&report);
  Ok((report, verified))
}

fn reconcile(
  candidate_path: &Path,
  native_path: &Path,
  schema: NativeSchemaDefinition,
  work: &Path,
) -> Result<NativeReconciliationReport, NativeDatabaseError> {
  // Independent DuckDB instances need independent spill directories.
  let candidate_spill = work.join("spill-candidate");
  let native_spill = work.join("spill-native");
  for path in [&candidate_spill, &native_spill] {
    std::fs::create_dir(path).map_err(|error| {
      NativeDatabaseError::finalization("create a reconciliation spill directory", error)
    })?;
  }

  let mut epoch = EpochMilliseconds::open()?;
  let source_schema_sha256;
  let previous_source_schema_sha256;
  let mut tables = Vec::with_capacity(schema.tables.len());
  // Scoped so both connections - and with them the candidate file inside the
  // work directory - are released before the database is reopened for
  // verification and before the work directory is removed. Windows cannot
  // reopen or delete a file another DuckDB instance still holds; an early
  // return from inside this block drops them just the same.
  {
    let candidate =
      open_database(candidate_path, AccessMode::ReadOnly, &candidate_spill)?;
    source_schema_sha256 = read_candidate_provenance(&candidate)?;
    let native = open_database(native_path, AccessMode::ReadWrite, &native_spill)?;
    previous_source_schema_sha256 = require_reconcilable(&native, &schema)?;
    require_every_candidate_table_is_declared(&candidate, &schema)?;

    // Every plan and staging table is prepared before the transaction opens:
    // DuckDB temporary-table DDL does not belong inside the one transaction
    // whose all-or-nothing guarantee this module rests on.
    let mut prepared = Vec::with_capacity(schema.tables.len());
    for table in schema.tables {
      prepared.push(prepare_table(&candidate, &native, &schema, table)?);
    }
    for table in &prepared {
      native
        .execute_batch(&create_staging_sql(&table.row_staging, &table.plan))
        .map_err(|error| {
          NativeDatabaseError::duckdb(
            "create the reconciliation row staging table",
            error,
          )
        })?;
      native
        .execute_batch(&create_staging_sql(&table.key_staging, &table.key_plan))
        .map_err(|error| {
          NativeDatabaseError::duckdb(
            "create the reconciliation key staging table",
            error,
          )
        })?;
    }

    native.execute_batch("BEGIN TRANSACTION").map_err(|error| {
      NativeDatabaseError::duckdb("begin the reconciliation transaction", error)
    })?;
    let result = (|| -> Result<(), NativeDatabaseError> {
      for table in &prepared {
        tables.push(reconcile_table(&candidate, &native, &mut epoch, table)?);
      }
      rewrite_identities(&candidate, &native, &schema)?;
      let total_rows = tables.iter().try_fold(
        0_u64,
        |total, table: &NativeReconciliationTableReport| {
          total.checked_add(table.candidate_rows).ok_or_else(|| {
            NativeDatabaseError::finalization(
              "count the reconciled rows",
              "total row count overflowed u64",
            )
          })
        },
      )?;
      rewrite_metadata(&native, candidate_path, &source_schema_sha256, total_rows)
    })();
    if let Err(error) = result {
      let _ = native.execute_batch("ROLLBACK");
      return Err(error);
    }
    native.execute_batch("COMMIT").map_err(|error| {
      NativeDatabaseError::duckdb("commit the reconciliation transaction", error)
    })?;
    for table in &prepared {
      for staging in [&table.row_staging, &table.key_staging] {
        native
          .execute_batch(&format!(
            "DROP TABLE temp.main.{}",
            quote_identifier(staging)
          ))
          .map_err(|error| {
            NativeDatabaseError::duckdb("drop a reconciliation staging table", error)
          })?;
      }
    }
    native.execute_batch("CHECKPOINT").map_err(|error| {
      NativeDatabaseError::duckdb("checkpoint the reconciled database", error)
    })?;
  }
  require_no_wal(native_path)?;

  // Reuses the spill the closed read-write instance had: sequential, never
  // concurrent, which is the same order finalization verifies in.
  verify_reconciled(native_path, &native_spill, &schema, &mut tables)?;

  // Measured only after `verify_reconciled` returned and dropped its
  // connection, so nothing holds the file.
  let native_bytes = std::fs::metadata(native_path)
    .map_err(|error| {
      NativeDatabaseError::finalization("measure the reconciled database", error)
    })?
    .len();
  let total_rows = tables.iter().map(|table| table.reopened_rows).sum();
  Ok(NativeReconciliationReport {
    native_database_path: native_path.to_owned(),
    state: FINALIZED_UNSELECTED.to_owned(),
    schema_version: schema.version,
    source_schema_sha256,
    previous_source_schema_sha256,
    inserted_rows: tables.iter().map(|table| table.inserted_rows).sum(),
    updated_rows: tables.iter().map(|table| table.updated_rows).sum(),
    deleted_rows: tables.iter().map(|table| table.deleted_rows).sum(),
    tables,
    total_rows,
    native_bytes,
  })
}

/// Refuse a database that is not a finalized, unselected file this build can
/// run, and return the source digest it recorded.
///
/// A selected database is refused rather than merged into: once it is
/// authoritative, SQLite is no longer the side that moved on, and reconciling
/// would overwrite the rows the application has since written natively.
fn require_reconcilable(
  native: &Connection,
  schema: &NativeSchemaDefinition,
) -> Result<String, NativeDatabaseError> {
  let (state, version, storage_version, recorded): (String, i64, String, String) = native
    .query_row(
      &format!(
        "SELECT state, schema_version, storage_version, source_schema_sha256 FROM {}",
        quote_identifier(NATIVE_METADATA_TABLE)
      ),
      [],
      |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )
    .map_err(|error| {
      NativeDatabaseError::duckdb("read the native metadata to reconcile", error)
    })?;
  if state != FINALIZED_UNSELECTED {
    return Err(NativeDatabaseError::UnexpectedState {
      operation: "reconciled",
      state,
      expected: FINALIZED_UNSELECTED,
    });
  }
  let version = u32::try_from(version).unwrap_or(u32::MAX);
  if version != schema.version {
    return Err(NativeDatabaseError::IncompatibleSchema {
      expected: schema.version,
      actual: version,
    });
  }
  verify_storage_version(native, &storage_version)?;
  Ok(recorded)
}

/// Everything one table's merge needs, resolved before the transaction opens.
struct PreparedTable {
  name: String,
  /// The finalized columns, in native column order, and where each value comes
  /// from in a candidate row.
  plan: Vec<ColumnPlan>,
  /// The primary-key columns, as staging columns for the delete key table.
  key_plan: Vec<ColumnPlan>,
  /// Positions of the primary-key columns within a finalized row.
  key_indices: Vec<usize>,
  key_columns: Vec<String>,
  /// The candidate projection: the source columns in plan order, then the
  /// snapshot ordinal, so decoded cells stay index-aligned with `plan`.
  candidate_projection: Vec<ReadColumn>,
  native_projection: Vec<ReadColumn>,
  row_staging: String,
  key_staging: String,
}

fn prepare_table(
  candidate: &Connection,
  native: &Connection,
  schema: &NativeSchemaDefinition,
  table: &str,
) -> Result<PreparedTable, NativeDatabaseError> {
  let candidate_columns = read_columns(candidate, table)?;
  if candidate_columns.is_empty() {
    return Err(NativeDatabaseError::SchemaMismatch {
      table: table.to_owned(),
      detail: "the candidate has no such table".to_owned(),
    });
  }
  let ordinal = candidate_columns
    .iter()
    .find(|column| column.name == SOURCE_ORDINAL_COLUMN)
    .ok_or_else(|| NativeDatabaseError::SchemaMismatch {
      table: table.to_owned(),
      detail: format!("the candidate table has no {SOURCE_ORDINAL_COLUMN} column"),
    })?
    .clone();
  let source_columns = candidate_columns
    .into_iter()
    .filter(|column| column.name != SOURCE_ORDINAL_COLUMN)
    .collect::<Vec<_>>();
  let native_columns = read_columns(native, table)?;
  let plan = plan_columns(schema, table, &source_columns, &native_columns)?;

  let key_columns = read_primary_key(native, table)?;
  let key_indices = key_columns
    .iter()
    .map(|key| {
      plan
        .iter()
        .position(|column| &column.name == key)
        .ok_or_else(|| NativeDatabaseError::SchemaMismatch {
          table: table.to_owned(),
          detail: format!("primary key column {key} is not a finalized column"),
        })
    })
    .collect::<Result<Vec<_>, _>>()?;

  let mut key_plan = Vec::with_capacity(key_indices.len());
  for index in &key_indices {
    let column = &plan[*index];
    // Both sides are read in the same key order, so the two orderings have to
    // be the same ordering. A tagged numeric key would be ordered by DuckDB's
    // union rules on one side and by a plain type on the other, and a key the
    // candidate stores under a different storage type would not compare at all.
    if matches!(column.kind, NativeColumnKind::TaggedNumeric) {
      return Err(NativeDatabaseError::SchemaMismatch {
        table: table.to_owned(),
        detail: format!(
          "primary key column {} is a tagged numeric, which has no single \
           comparable storage type to merge both sides by",
          column.name
        ),
      });
    }
    let ColumnSource::Candidate(source_index) = column.source else {
      return Err(NativeDatabaseError::SchemaMismatch {
        table: table.to_owned(),
        detail: format!(
          "primary key column {} is derived rather than copied, so the \
           candidate cannot be read in that key order",
          column.name
        ),
      });
    };
    let source = &source_columns[source_index];
    if source.kind != column.kind {
      return Err(NativeDatabaseError::SchemaMismatch {
        table: table.to_owned(),
        detail: format!(
          "primary key column {} is {} in the candidate and {} in the native \
           schema, so the two sides cannot be merged in one key order",
          column.name,
          source.kind.sql(),
          column.kind.sql()
        ),
      });
    }
    key_plan.push(ColumnPlan {
      name: column.name.clone(),
      kind: column.kind,
      nullable: false,
      source: ColumnSource::Candidate(source_index),
    });
  }

  let candidate_projection = source_columns
    .iter()
    .chain(std::iter::once(&ordinal))
    .map(read_column)
    .collect::<Vec<_>>();
  let native_projection = native_columns.iter().map(read_column).collect::<Vec<_>>();

  Ok(PreparedTable {
    name: table.to_owned(),
    plan,
    key_plan,
    key_indices,
    key_columns,
    candidate_projection,
    native_projection,
    row_staging: format!("__hv_reconcile_rows_{table}"),
    key_staging: format!("__hv_reconcile_keys_{table}"),
  })
}

fn read_column(column: &TableColumn) -> ReadColumn {
  ReadColumn {
    name: column.name.clone(),
    kind: column.kind,
  }
}

/// Merge one table's two key-ordered streams and apply the differences.
fn reconcile_table(
  candidate: &Connection,
  native: &Connection,
  epoch: &mut EpochMilliseconds,
  table: &PreparedTable,
) -> Result<NativeReconciliationTableReport, NativeDatabaseError> {
  let mut candidate_rows = CandidateRows::new(PagedReader::new(
    candidate,
    &table.name,
    table.candidate_projection.clone(),
    table.key_columns.clone(),
  )?);
  let mut native_rows = NativeRows::new(PagedReader::new(
    native,
    &table.name,
    table.native_projection.clone(),
    table.key_columns.clone(),
  )?);

  let mut change = ChangeBuffer::new();
  let mut expected = RowMultisetDigest::default();
  let mut report = NativeReconciliationTableReport {
    name: table.name.clone(),
    candidate_rows: 0,
    inserted_rows: 0,
    updated_rows: 0,
    deleted_rows: 0,
    unchanged_rows: 0,
    reopened_rows: 0,
    expected_digest: String::new(),
    reopened_digest: String::new(),
  };

  let mut wanted = candidate_rows.next(epoch, table)?;
  let mut held = native_rows.next()?;
  loop {
    match (&wanted, &held) {
      (None, None) => break,
      (Some((row, ordinal)), None) => {
        expected.add_row(row);
        report.candidate_rows += 1;
        report.inserted_rows += 1;
        change.push(native, table, Some((row, *ordinal)), row)?;
        wanted = candidate_rows.next(epoch, table)?;
      }
      (None, Some(row)) => {
        report.deleted_rows += 1;
        change.push(native, table, None, row)?;
        held = native_rows.next()?;
      }
      (Some((row, ordinal)), Some(current)) => {
        match compare_keys(
          &key_of(row, &table.key_indices),
          &key_of(current, &table.key_indices),
        ) {
          std::cmp::Ordering::Less => {
            expected.add_row(row);
            report.candidate_rows += 1;
            report.inserted_rows += 1;
            change.push(native, table, Some((row, *ordinal)), row)?;
            wanted = candidate_rows.next(epoch, table)?;
          }
          std::cmp::Ordering::Greater => {
            report.deleted_rows += 1;
            change.push(native, table, None, current)?;
            held = native_rows.next()?;
          }
          std::cmp::Ordering::Equal => {
            expected.add_row(row);
            report.candidate_rows += 1;
            if row == current {
              report.unchanged_rows += 1;
            } else {
              report.updated_rows += 1;
              change.push(native, table, Some((row, *ordinal)), row)?;
            }
            wanted = candidate_rows.next(epoch, table)?;
            held = native_rows.next()?;
          }
        }
      }
    }
  }
  change.flush(native, table)?;
  report.expected_digest = expected.encode();
  Ok(report)
}

fn key_of(row: &[Cell], indices: &[usize]) -> Vec<Cell> {
  indices.iter().map(|index| row[*index].clone()).collect()
}

/// Order two keys the way DuckDB orders the `ORDER BY` both readers page on.
///
/// Only the storage types a primary key may use reach this: integers compare
/// numerically and text compares by its UTF-8 bytes, which is DuckDB's default
/// collation and Rust's `str` ordering alike. Anything else would mean the two
/// streams were paged in different orders, so it is reported rather than
/// guessed at.
fn compare_keys(left: &[Cell], right: &[Cell]) -> std::cmp::Ordering {
  for (left, right) in left.iter().zip(right) {
    let ordering = match (left, right) {
      (Cell::Integer(left), Cell::Integer(right)) => left.cmp(right),
      (Cell::Text(left), Cell::Text(right)) => left.as_bytes().cmp(right.as_bytes()),
      (Cell::Blob(left), Cell::Blob(right)) => left.cmp(right),
      (Cell::Real(left), Cell::Real(right)) => f64::from_bits(*left)
        .partial_cmp(&f64::from_bits(*right))
        .unwrap_or(std::cmp::Ordering::Equal),
      _ => std::cmp::Ordering::Equal,
    };
    if ordering != std::cmp::Ordering::Equal {
      return ordering;
    }
  }
  std::cmp::Ordering::Equal
}

/// The candidate side of the merge, already mapped into its finalized
/// representation.
struct CandidateRows<'a> {
  reader: PagedReader<'a>,
  page: Vec<(Vec<Cell>, u64)>,
  index: usize,
}

impl<'a> CandidateRows<'a> {
  fn new(reader: PagedReader<'a>) -> Self {
    Self {
      reader,
      page: Vec::new(),
      index: 0,
    }
  }

  fn next(
    &mut self,
    epoch: &mut EpochMilliseconds,
    table: &PreparedTable,
  ) -> Result<Option<(Vec<Cell>, u64)>, NativeDatabaseError> {
    loop {
      if self.index < self.page.len() {
        let row = std::mem::take(&mut self.page[self.index]);
        self.index += 1;
        return Ok(Some(row));
      }
      let Some(page) = self.reader.next_page()? else {
        return Ok(None);
      };
      let ordinals = page
        .iter()
        .map(|cells| match cells.last() {
          Some(Cell::Integer(value)) => u64::try_from(*value).unwrap_or(u64::MAX),
          _ => u64::MAX,
        })
        .collect::<Vec<_>>();
      // Finalization already reports the unconvertible-stamp count per table;
      // reconciliation re-derives the same keys and does not report it again.
      let mut unconvertible = 0_u64;
      let derived = derive_epoch_milliseconds(
        epoch,
        &table.name,
        &table.plan,
        &page,
        &ordinals,
        &mut unconvertible,
      )?;
      self.page = page
        .iter()
        .enumerate()
        .map(|(row_index, cells)| {
          let row = table
            .plan
            .iter()
            .enumerate()
            .map(|(column_index, column)| match column.source {
              ColumnSource::Candidate(index) => cells[index].clone(),
              ColumnSource::EpochMilliseconds(_) => derived
                .get(&(row_index, column_index))
                .cloned()
                .unwrap_or(Cell::Null),
            })
            .collect::<Vec<_>>();
          (row, ordinals[row_index])
        })
        .collect();
      self.index = 0;
    }
  }
}

/// The finalized side of the merge, read back in the same key order.
struct NativeRows<'a> {
  reader: PagedReader<'a>,
  page: Vec<Vec<Cell>>,
  index: usize,
}

impl<'a> NativeRows<'a> {
  fn new(reader: PagedReader<'a>) -> Self {
    Self {
      reader,
      page: Vec::new(),
      index: 0,
    }
  }

  fn next(&mut self) -> Result<Option<Vec<Cell>>, NativeDatabaseError> {
    loop {
      if self.index < self.page.len() {
        let row = std::mem::take(&mut self.page[self.index]);
        self.index += 1;
        return Ok(Some(row));
      }
      let Some(page) = self.reader.next_page()? else {
        return Ok(None);
      };
      self.page = page;
      self.index = 0;
    }
  }
}

/// Accumulates one flush worth of differences.
///
/// Every changed key is staged for deletion, including a key that is only being
/// inserted: deleting first and inserting after makes the flush idempotent and
/// keeps the result correct even if the two paged streams ever disagreed about
/// an ordering, at the cost of one no-op delete per inserted row.
struct ChangeBuffer {
  key_values: Vec<Value>,
  key_rows: usize,
  row_values: Vec<Value>,
  rows: usize,
}

impl ChangeBuffer {
  fn new() -> Self {
    Self {
      key_values: Vec::new(),
      key_rows: 0,
      row_values: Vec::new(),
      rows: 0,
    }
  }

  /// Stage one difference: `insert` carries the finalized row to write (absent
  /// for a pure deletion), and `key_source` is the row whose key is removed.
  fn push(
    &mut self,
    native: &Connection,
    table: &PreparedTable,
    insert: Option<(&Vec<Cell>, u64)>,
    key_source: &[Cell],
  ) -> Result<(), NativeDatabaseError> {
    for (index, column) in table.key_plan.iter().enumerate() {
      stage_cell(
        &table.name,
        column,
        insert.map_or(u64::MAX, |(_, ordinal)| ordinal),
        &key_source[table.key_indices[index]],
        &mut self.key_values,
      )?;
    }
    self.key_values.push(Value::UBigInt(self.key_rows as u64));
    self.key_rows += 1;

    if let Some((row, ordinal)) = insert {
      for (index, column) in table.plan.iter().enumerate() {
        stage_cell(
          &table.name,
          column,
          ordinal,
          &row[index],
          &mut self.row_values,
        )?;
      }
      self.row_values.push(Value::UBigInt(self.rows as u64));
      self.rows += 1;
    }
    if self.key_rows as u64 >= COPY_BATCH_ROWS {
      self.flush(native, table)?;
    }
    Ok(())
  }

  fn flush(
    &mut self,
    native: &Connection,
    table: &PreparedTable,
  ) -> Result<(), NativeDatabaseError> {
    if self.key_rows == 0 {
      return Ok(());
    }
    append_staging(
      native,
      &table.key_staging,
      &table.key_plan,
      self.key_rows,
      std::mem::take(&mut self.key_values),
    )?;
    native
      .execute_batch(&delete_by_staged_key_sql(table))
      .map_err(|error| {
        NativeDatabaseError::duckdb("delete the reconciled rows being replaced", error)
      })?;
    if self.rows > 0 {
      append_staging(
        native,
        &table.row_staging,
        &table.plan,
        self.rows,
        std::mem::take(&mut self.row_values),
      )?;
      native
        .execute_batch(&insert_from_staging_sql(
          &table.name,
          &table.row_staging,
          &table.plan,
        ))
        .map_err(|error| {
          NativeDatabaseError::duckdb("insert a reconciled page", error)
        })?;
    }
    for staging in [&table.key_staging, &table.row_staging] {
      native
        .execute_batch(&format!(
          "DELETE FROM temp.main.{}",
          quote_identifier(staging)
        ))
        .map_err(|error| {
          NativeDatabaseError::duckdb("clear a reconciliation staging table", error)
        })?;
    }
    self.key_rows = 0;
    self.rows = 0;
    Ok(())
  }
}

/// Staging column names are `c0000`-shaped, so they can never collide with a
/// domain column and the correlated reference needs no alias.
fn delete_by_staged_key_sql(table: &PreparedTable) -> String {
  let predicate = table
    .key_plan
    .iter()
    .enumerate()
    .map(|(index, column)| {
      format!(
        "\"c{index:04}\" = {}.{}",
        quote_identifier(&table.name),
        quote_identifier(&column.name)
      )
    })
    .collect::<Vec<_>>()
    .join(" AND ");
  format!(
    "DELETE FROM {} WHERE EXISTS (SELECT 1 FROM temp.main.{} WHERE {predicate})",
    quote_identifier(&table.name),
    quote_identifier(&table.key_staging)
  )
}

/// Replace the imported identity high-water marks with the new candidate's.
///
/// SQLite's allocator state moved on while the application kept writing, and a
/// native database that kept the old marks could hand out an id the source has
/// already used.
fn rewrite_identities(
  candidate: &Connection,
  native: &Connection,
  schema: &NativeSchemaDefinition,
) -> Result<(), NativeDatabaseError> {
  native
    .execute_batch(&format!(
      "DELETE FROM {}",
      quote_identifier(NATIVE_IDENTITY_TABLE)
    ))
    .map_err(|error| {
      NativeDatabaseError::duckdb("clear the native identity metadata", error)
    })?;
  write_identities(candidate, native, schema)
}

/// The file is now a copy of the new candidate, so it records that candidate's
/// provenance rather than the snapshot it was originally finalized from, and
/// the `reconciled` flag selection requires is raised in the same transaction.
fn rewrite_metadata(
  native: &Connection,
  candidate_path: &Path,
  source_schema_sha256: &str,
  total_rows: u64,
) -> Result<(), NativeDatabaseError> {
  let rows = i64::try_from(total_rows).map_err(|_| {
    NativeDatabaseError::finalization(
      "write the reconciled native metadata",
      "total row count does not fit a signed 64-bit value",
    )
  })?;
  native
    .execute(
      &format!(
        "UPDATE {} SET source_candidate_path = ?, source_schema_sha256 = ?, \
         source_rows = ?, reconciled = true",
        quote_identifier(NATIVE_METADATA_TABLE)
      ),
      duckdb::params![
        candidate_path.to_string_lossy().as_ref(),
        source_schema_sha256,
        rows
      ],
    )
    .map(|_| ())
    .map_err(|error| {
      NativeDatabaseError::duckdb("write the reconciled native metadata", error)
    })
}

/// Reopen the closed file and require every table to equal the candidate it was
/// reconciled against.
fn verify_reconciled(
  native_path: &Path,
  spill: &Path,
  schema: &NativeSchemaDefinition,
  tables: &mut [NativeReconciliationTableReport],
) -> Result<(), NativeDatabaseError> {
  let connection = open_database(native_path, AccessMode::ReadOnly, spill)?;
  let (state, version, storage_version): (String, i64, String) = connection
    .query_row(
      &format!(
        "SELECT state, schema_version, storage_version FROM {}",
        quote_identifier(NATIVE_METADATA_TABLE)
      ),
      [],
      |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .map_err(|error| {
      NativeDatabaseError::duckdb("read the reopened reconciled metadata", error)
    })?;
  verify_storage_version(&connection, &storage_version)?;
  if state != FINALIZED_UNSELECTED || version != i64::from(schema.version) {
    return Err(NativeDatabaseError::Verification {
      message: "the reopened native metadata does not match what was written".to_owned(),
    });
  }

  for table in tables {
    let columns = read_columns(&connection, &table.name)?;
    let key_columns = read_primary_key(&connection, &table.name)?;
    let mut reader = PagedReader::new(
      &connection,
      &table.name,
      columns.iter().map(read_column).collect(),
      key_columns,
    )?;
    let mut digest = RowMultisetDigest::default();
    while let Some(page) = reader.next_page()? {
      for row in page {
        digest.add_row(&row);
      }
    }
    table.reopened_rows = count_rows(&connection, &table.name)?;
    table.reopened_digest = digest.encode();
    if table.reopened_rows != table.candidate_rows
      || digest.rows() != table.candidate_rows
      || table.reopened_digest != table.expected_digest
    {
      return Err(NativeDatabaseError::Verification {
        message: format!(
          "{} read back {} rows with digest {} after reconciliation, but the \
           candidate holds {} rows with digest {}",
          table.name,
          table.reopened_rows,
          table.reopened_digest,
          table.candidate_rows,
          table.expected_digest
        ),
      });
    }
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn keys_order_integers_numerically_and_text_by_bytes() {
    use std::cmp::Ordering;

    assert_eq!(
      compare_keys(&[Cell::Integer(-2)], &[Cell::Integer(10)]),
      Ordering::Less
    );
    assert_eq!(
      compare_keys(&[Cell::Text("Z".to_owned())], &[Cell::Text("a".to_owned())]),
      Ordering::Less
    );
    // A composite key falls through to the later columns only on a tie.
    assert_eq!(
      compare_keys(
        &[
          Cell::Text("2026-09-01".to_owned()),
          Cell::Text("CPU".to_owned())
        ],
        &[
          Cell::Text("2026-09-01".to_owned()),
          Cell::Text("GPU".to_owned())
        ]
      ),
      Ordering::Less
    );
    assert_eq!(
      compare_keys(
        &[
          Cell::Text("2026-09-02".to_owned()),
          Cell::Text("CPU".to_owned())
        ],
        &[
          Cell::Text("2026-09-01".to_owned()),
          Cell::Text("GPU".to_owned())
        ]
      ),
      Ordering::Greater
    );
  }
}
