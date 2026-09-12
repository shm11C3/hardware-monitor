//! Turns one immutable #2088 candidate into a finalized native database.
//!
//! The candidate represents the SQLite source honestly but not usefully: its
//! column types follow whatever storage classes the snapshot happened to
//! contain, and it carries snapshot-validation metadata rather than domain
//! constraints. Finalization copies it once into the App-supplied stable schema
//! ([`NativeSchemaDefinition`]), fills the derived timestamp keys, and imports
//! the identity high-water state.
//!
//! Nothing here publishes or selects the result. A finalized file is still
//! unselected: SQLite stays authoritative until the lifecycle change that
//! chooses a backend.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use duckdb::types::Value;
use duckdb::{AccessMode, Config, Connection, appender_params_from_iter};

use super::NativeDatabaseError;
use super::cell::{Cell, NativeColumnKind, RowMultisetDigest, quote_identifier};
use super::epoch::EpochMilliseconds;
use super::paging::{PagedReader, ReadColumn};
use super::schema::{NativeIdentityMode, NativeSchemaDefinition};

pub(super) const NATIVE_METADATA_TABLE: &str = "__hv_native_metadata";
pub(super) const NATIVE_IDENTITY_TABLE: &str = "__hv_native_identities";
pub(super) const FINALIZED_UNSELECTED: &str = "finalized_unselected";
const SNAPSHOT_METADATA_TABLE: &str = "__hv_snapshot_metadata";
const SOURCE_ORDINAL_COLUMN: &str = "__hv_source_ordinal";
const SQLITE_SEQUENCE_TABLE: &str = "sqlite_sequence";
const SQLX_MIGRATIONS_TABLE: &str = "_sqlx_migrations";
const WORK_PREFIX: &str = ".hardwarevisualizer-duckdb-finalize-";

/// Candidate tables that carry no domain rows, so the stable schema does not
/// declare them: sqlx's migration ledger and SQLite's allocator state are
/// carried forward as the recorded source schema digest and the imported
/// identity high-water marks, and the snapshot metadata belongs to the
/// candidate.
const NON_DOMAIN_CANDIDATE_TABLES: &[&str] = &[
  SNAPSHOT_METADATA_TABLE,
  SQLITE_SEQUENCE_TABLE,
  SQLX_MIGRATIONS_TABLE,
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeTableReport {
  pub name: String,
  pub candidate_rows: u64,
  pub copied_rows: u64,
  pub reopened_rows: u64,
  /// Order-independent digest of the rows written, in their finalized
  /// representation.
  pub copied_digest: String,
  /// The same digest recomputed after the database was closed and reopened.
  pub reopened_digest: String,
  /// Rows whose source timestamp text is present but which SQLite cannot read
  /// as an instant, so their derived epoch key is NULL and no range query can
  /// bucket them - in either engine.
  ///
  /// No writer this application ships produces such a stamp, and a conversion
  /// is the one moment the whole archive is read, so the count is reported
  /// here rather than discovered later one query at a time. It is
  /// informational: a non-zero count does not fail the conversion, because the
  /// rows themselves are copied intact and every other query still sees them.
  pub unconvertible_timestamps: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeFinalizationReport {
  pub source_candidate_path: PathBuf,
  pub finalized_database_path: PathBuf,
  pub state: String,
  pub schema_version: u32,
  /// The candidate's record of the SQLite schema it was copied from, carried
  /// forward so a finalized file can still name its origin.
  pub source_schema_sha256: String,
  pub tables: Vec<NativeTableReport>,
  pub total_rows: u64,
  pub finalized_bytes: u64,
}

/// Build a finalized native file from an immutable candidate.
///
/// Neither the candidate nor the SQLite source it came from is modified, and a
/// destination that already exists is refused rather than replaced. Everything
/// is written inside a work directory next to `destination` and published by a
/// hard link only after the closed file has been reopened and verified, so a
/// failure leaves no partial destination behind.
pub async fn finalize_candidate_database(
  source_candidate: &Path,
  destination: &Path,
  schema: NativeSchemaDefinition,
) -> Result<NativeFinalizationReport, NativeDatabaseError> {
  validate_paths(source_candidate, destination)?;
  let candidate = source_candidate.to_owned();
  let destination = destination.to_owned();
  tokio::task::spawn_blocking(move || finalize(&candidate, &destination, schema))
    .await
    .map_err(|error| NativeDatabaseError::Worker {
      message: error.to_string(),
    })?
}

fn validate_paths(
  candidate: &Path,
  destination: &Path,
) -> Result<(), NativeDatabaseError> {
  let metadata =
    fs::metadata(candidate).map_err(|_| NativeDatabaseError::CandidateUnavailable {
      path: candidate.to_owned(),
    })?;
  if !metadata.is_file() {
    return Err(NativeDatabaseError::CandidateUnavailable {
      path: candidate.to_owned(),
    });
  }
  match fs::symlink_metadata(destination) {
    Ok(_) => Err(NativeDatabaseError::DestinationExists {
      path: destination.to_owned(),
    }),
    Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
    Err(error) => Err(NativeDatabaseError::finalization(
      "inspect the finalization destination",
      error,
    )),
  }
}

fn finalize(
  candidate_path: &Path,
  destination: &Path,
  schema: NativeSchemaDefinition,
) -> Result<NativeFinalizationReport, NativeDatabaseError> {
  let parent = destination
    .parent()
    .filter(|path| !path.as_os_str().is_empty())
    .ok_or_else(|| {
      NativeDatabaseError::finalization(
        "reserve the finalization work directory",
        "destination must have a parent directory",
      )
    })?;
  if !parent.is_dir() {
    return Err(NativeDatabaseError::finalization(
      "reserve the finalization work directory",
      format!("destination parent does not exist: {}", parent.display()),
    ));
  }

  // Declared before every connection so the connections drop first: Windows
  // cannot remove an open database file.
  let work = tempfile::Builder::new()
    .prefix(WORK_PREFIX)
    .tempdir_in(parent)
    .map_err(|error| {
      NativeDatabaseError::finalization("reserve the finalization work directory", error)
    })?;
  let database_path = work.path().join("finalized.duckdb");
  // Independent DuckDB instances need independent spill directories.
  let candidate_spill = work.path().join("spill-candidate");
  let destination_spill = work.path().join("spill-destination");
  for path in [&candidate_spill, &destination_spill] {
    fs::create_dir(path).map_err(|error| {
      NativeDatabaseError::finalization("create a finalization spill directory", error)
    })?;
  }

  let mut epoch = EpochMilliseconds::open()?;
  let copied;
  let source_schema_sha256;
  {
    let candidate =
      open_database(candidate_path, AccessMode::ReadOnly, &candidate_spill)?;
    source_schema_sha256 = read_candidate_provenance(&candidate)?;
    require_every_candidate_table_is_declared(&candidate, &schema)?;
    let destination_database =
      open_database(&database_path, AccessMode::ReadWrite, &destination_spill)?;
    destination_database
      .execute_batch(schema.sql)
      .map_err(|error| NativeDatabaseError::duckdb("create the native schema", error))?;
    destination_database
      .execute_batch(&format!(
        "CREATE TABLE {} (state VARCHAR NOT NULL, schema_version BIGINT NOT NULL, \
         source_candidate_path VARCHAR NOT NULL, source_schema_sha256 VARCHAR NOT NULL, \
         source_rows BIGINT NOT NULL); \
         CREATE TABLE {} (table_name VARCHAR PRIMARY KEY, column_name VARCHAR NOT NULL, \
         mode VARCHAR NOT NULL, high_water BIGINT NOT NULL)",
        quote_identifier(NATIVE_METADATA_TABLE),
        quote_identifier(NATIVE_IDENTITY_TABLE)
      ))
      .map_err(|error| {
        NativeDatabaseError::duckdb("create the native metadata tables", error)
      })?;

    let mut tables = Vec::with_capacity(schema.tables.len());
    for table in schema.tables {
      tables.push(copy_table(
        &candidate,
        &destination_database,
        &mut epoch,
        &schema,
        table,
      )?);
    }
    let total_rows = tables.iter().try_fold(0_u64, |total, table| {
      total.checked_add(table.rows).ok_or_else(|| {
        NativeDatabaseError::finalization(
          "count the finalized rows",
          "total row count overflowed u64",
        )
      })
    })?;

    write_identities(&candidate, &destination_database, &schema)?;
    write_metadata(
      &destination_database,
      &schema,
      candidate_path,
      &source_schema_sha256,
      total_rows,
    )?;
    destination_database
      .execute_batch("CHECKPOINT")
      .map_err(|error| {
        NativeDatabaseError::duckdb("checkpoint the finalized database", error)
      })?;
    copied = tables;
  }
  require_no_wal(&database_path)?;

  let tables = verify_finalized(
    &database_path,
    &destination_spill,
    &schema,
    &source_schema_sha256,
    &copied,
  )?;
  require_no_wal(&database_path)?;

  let file = OpenOptions::new()
    .read(true)
    .write(true)
    .open(&database_path)
    .map_err(|error| {
      NativeDatabaseError::finalization("open the finalized database for sync", error)
    })?;
  file.sync_all().map_err(|error| {
    NativeDatabaseError::finalization("sync the finalized database", error)
  })?;
  let finalized_bytes = file
    .metadata()
    .map_err(|error| {
      NativeDatabaseError::finalization("measure the finalized database", error)
    })?
    .len();
  drop(file);
  fs::hard_link(&database_path, destination).map_err(|error| {
    NativeDatabaseError::finalization(
      "publish the finalized database without replacement",
      format!("{}: {error}", destination.display()),
    )
  })?;
  drop(work);

  let total_rows = tables.iter().map(|table| table.copied_rows).sum();
  Ok(NativeFinalizationReport {
    source_candidate_path: candidate_path.to_owned(),
    finalized_database_path: destination.to_owned(),
    state: FINALIZED_UNSELECTED.to_owned(),
    schema_version: schema.version,
    source_schema_sha256,
    tables,
    total_rows,
    finalized_bytes,
  })
}

/// Refuse a candidate that carries a domain table the stable schema never
/// declared.
///
/// [`copy_table`] walks the App-declared table list, so a table the list forgot
/// would simply not be copied - the table-level form of the missing-column
/// failure [`plan_columns`] already refuses, and just as silent a loss of
/// history. Checked before anything is written, so a schema definition that has
/// fallen behind the migration set produces no partial file.
fn require_every_candidate_table_is_declared(
  candidate: &Connection,
  schema: &NativeSchemaDefinition,
) -> Result<(), NativeDatabaseError> {
  let mut statement = candidate
    .prepare(
      "SELECT table_name FROM information_schema.tables \
       WHERE table_schema = 'main' ORDER BY table_name",
    )
    .map_err(|error| {
      NativeDatabaseError::duckdb("prepare the candidate table read", error)
    })?;
  let names = statement
    .query_map([], |row| row.get::<_, String>(0))
    .map_err(|error| NativeDatabaseError::duckdb("read the candidate tables", error))?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|error| NativeDatabaseError::duckdb("read the candidate tables", error))?;
  for name in names {
    if NON_DOMAIN_CANDIDATE_TABLES.contains(&name.as_str())
      || schema.tables.contains(&name.as_str())
    {
      continue;
    }
    return Err(NativeDatabaseError::SchemaMismatch {
      table: name,
      detail: "the candidate carries this table but the native schema does not \
               declare it, so finalizing would drop its rows"
        .to_owned(),
    });
  }
  Ok(())
}

struct CopiedTable {
  name: String,
  candidate_rows: u64,
  rows: u64,
  digest: RowMultisetDigest,
  unconvertible_timestamps: u64,
}

/// One finalized column and where its value comes from.
struct ColumnPlan {
  name: String,
  kind: NativeColumnKind,
  nullable: bool,
  source: ColumnSource,
}

enum ColumnSource {
  /// Copied verbatim from the candidate cell at this index.
  Candidate(usize),
  /// Derived from the stored timestamp text at this candidate index.
  EpochMilliseconds(usize),
}

fn copy_table(
  candidate: &Connection,
  destination: &Connection,
  epoch: &mut EpochMilliseconds,
  schema: &NativeSchemaDefinition,
  table: &str,
) -> Result<CopiedTable, NativeDatabaseError> {
  let candidate_columns = read_columns(candidate, table)?;
  if candidate_columns.is_empty() {
    return Err(NativeDatabaseError::SchemaMismatch {
      table: table.to_owned(),
      detail: "the candidate has no such table".to_owned(),
    });
  }
  if !candidate_columns
    .iter()
    .any(|column| column.name == SOURCE_ORDINAL_COLUMN)
  {
    return Err(NativeDatabaseError::SchemaMismatch {
      table: table.to_owned(),
      detail: format!("the candidate table has no {SOURCE_ORDINAL_COLUMN} column"),
    });
  }
  let source_columns = candidate_columns
    .iter()
    .filter(|column| column.name != SOURCE_ORDINAL_COLUMN)
    .cloned()
    .collect::<Vec<_>>();
  let destination_columns = read_columns(destination, table)?;
  let plan = plan_columns(schema, table, &source_columns, &destination_columns)?;

  let candidate_rows = count_rows(candidate, table)?;
  let staging = format!("__hv_finalize_stage_{table}");
  destination
    .execute_batch(&create_staging_sql(&staging, &plan))
    .map_err(|error| {
      NativeDatabaseError::duckdb("create the finalization staging table", error)
    })?;

  // The ordinal is read last so a decoded row's source cells stay index-aligned
  // with `source_columns`.
  let projection = source_columns
    .iter()
    .chain(
      candidate_columns
        .iter()
        .filter(|column| column.name == SOURCE_ORDINAL_COLUMN),
    )
    .map(|column| ReadColumn {
      name: column.name.clone(),
      kind: column.kind,
    })
    .collect::<Vec<_>>();
  let mut reader = PagedReader::new(
    candidate,
    table,
    projection,
    vec![SOURCE_ORDINAL_COLUMN.to_owned()],
  )?;

  let mut digest = RowMultisetDigest::default();
  let mut rows = 0_u64;
  let mut ordinal_base = 0_u64;
  let mut unconvertible_timestamps = 0_u64;
  destination
    .execute_batch("BEGIN TRANSACTION")
    .map_err(|error| {
      NativeDatabaseError::duckdb("begin the finalization transaction", error)
    })?;
  let result = (|| -> Result<(), NativeDatabaseError> {
    while let Some(page) = reader.next_page()? {
      let ordinals = page
        .iter()
        .map(|cells| match cells.last() {
          Some(Cell::Integer(value)) => u64::try_from(*value).unwrap_or(u64::MAX),
          _ => u64::MAX,
        })
        .collect::<Vec<_>>();
      let derived = derive_epoch_milliseconds(
        epoch,
        table,
        &plan,
        &page,
        &ordinals,
        &mut unconvertible_timestamps,
      )?;

      let mut values = Vec::new();
      for (row_index, cells) in page.iter().enumerate() {
        let ordinal = ordinals[row_index];
        let mut row_cells = Vec::with_capacity(plan.len());
        for (column_index, column) in plan.iter().enumerate() {
          let cell = match column.source {
            ColumnSource::Candidate(index) => cells[index].clone(),
            ColumnSource::EpochMilliseconds(_) => derived
              .get(&(row_index, column_index))
              .cloned()
              .unwrap_or(Cell::Null),
          };
          stage_cell(table, column, ordinal, &cell, &mut values)?;
          row_cells.push(cell);
        }
        values.push(Value::UBigInt(ordinal_base + row_index as u64));
        digest.add_row(&row_cells);
        rows = rows.checked_add(1).ok_or_else(|| {
          NativeDatabaseError::finalization(
            "copy a candidate table",
            "row count overflowed u64",
          )
        })?;
      }
      append_staging(destination, &staging, &plan, page.len(), values)?;
      destination
        .execute_batch(&insert_from_staging_sql(table, &staging, &plan))
        .map_err(|error| NativeDatabaseError::duckdb("insert a finalized page", error))?;
      destination
        .execute_batch(&format!(
          "DELETE FROM temp.main.{}",
          quote_identifier(&staging)
        ))
        .map_err(|error| {
          NativeDatabaseError::duckdb("clear the finalization staging table", error)
        })?;
      ordinal_base += page.len() as u64;
    }
    Ok(())
  })();
  if let Err(error) = result {
    let _ = destination.execute_batch("ROLLBACK");
    return Err(error);
  }
  destination.execute_batch("COMMIT").map_err(|error| {
    NativeDatabaseError::duckdb("commit the finalization transaction", error)
  })?;
  destination
    .execute_batch(&format!(
      "DROP TABLE temp.main.{}",
      quote_identifier(&staging)
    ))
    .map_err(|error| {
      NativeDatabaseError::duckdb("drop the finalization staging table", error)
    })?;

  if rows != candidate_rows {
    return Err(NativeDatabaseError::Verification {
      message: format!(
        "{table} holds {candidate_rows} candidate rows but {rows} were copied"
      ),
    });
  }
  Ok(CopiedTable {
    name: table.to_owned(),
    candidate_rows,
    rows,
    digest,
    unconvertible_timestamps,
  })
}

/// Convert the page's stored timestamp texts in one batch per derived column.
/// `unconvertible` accumulates the rows whose source text is present but which
/// SQLite cannot read, which is the only case where a derived key is NULL while
/// its source is not.
fn derive_epoch_milliseconds(
  epoch: &mut EpochMilliseconds,
  table: &str,
  plan: &[ColumnPlan],
  page: &[Vec<Cell>],
  ordinals: &[u64],
  unconvertible: &mut u64,
) -> Result<BTreeMap<(usize, usize), Cell>, NativeDatabaseError> {
  let mut converted = BTreeMap::new();
  for (column_index, column) in plan.iter().enumerate() {
    let ColumnSource::EpochMilliseconds(source_index) = column.source else {
      continue;
    };
    let mut texts = Vec::with_capacity(page.len());
    for (row_index, cells) in page.iter().enumerate() {
      match &cells[source_index] {
        Cell::Text(text) => texts.push(Some(text.clone())),
        Cell::Null => texts.push(None),
        other => {
          return Err(NativeDatabaseError::UnrepresentableCell {
            table: table.to_owned(),
            column: column.name.clone(),
            row_ordinal: ordinals[row_index],
            candidate: other.describe(),
            destination: "a derived epoch-millisecond key, which needs stored \
                          timestamp text"
              .to_owned(),
          });
        }
      }
    }
    for (row_index, milliseconds) in epoch.convert(&texts)?.into_iter().enumerate() {
      if milliseconds.is_none() && texts[row_index].is_some() {
        *unconvertible = unconvertible.saturating_add(1);
      }
      converted.insert(
        (row_index, column_index),
        milliseconds.map_or(Cell::Null, Cell::Integer),
      );
    }
  }
  Ok(converted)
}

fn plan_columns(
  schema: &NativeSchemaDefinition,
  table: &str,
  source_columns: &[TableColumn],
  destination_columns: &[TableColumn],
) -> Result<Vec<ColumnPlan>, NativeDatabaseError> {
  let mut plan = Vec::with_capacity(destination_columns.len());
  for column in destination_columns {
    let source = if let Some(derived) = schema.derived_column_for(table, &column.name) {
      let index = source_columns
        .iter()
        .position(|source| source.name == derived.source_column)
        .ok_or_else(|| NativeDatabaseError::SchemaMismatch {
          table: table.to_owned(),
          detail: format!(
            "derived column {} needs candidate column {}, which is absent",
            column.name, derived.source_column
          ),
        })?;
      ColumnSource::EpochMilliseconds(index)
    } else {
      let index = source_columns
        .iter()
        .position(|source| source.name == column.name)
        .ok_or_else(|| NativeDatabaseError::SchemaMismatch {
          table: table.to_owned(),
          detail: format!("the candidate has no column {}", column.name),
        })?;
      ColumnSource::Candidate(index)
    };
    plan.push(ColumnPlan {
      name: column.name.clone(),
      kind: column.kind,
      nullable: column.nullable,
      source,
    });
  }

  // Every candidate column must land somewhere: a column the stable schema
  // forgot would silently drop history.
  for source in source_columns {
    if !destination_columns
      .iter()
      .any(|column| column.name == source.name)
    {
      return Err(NativeDatabaseError::SchemaMismatch {
        table: table.to_owned(),
        detail: format!(
          "candidate column {} has no column in the native schema",
          source.name
        ),
      });
    }
  }
  Ok(plan)
}

/// Refuse, rather than coerce, any candidate cell the stable column cannot
/// hold. Widening an integer to a double would round beyond 2^53 and narrowing
/// a double to an integer would drop the fraction; both would make a stored
/// reading say something the source never said.
fn stage_cell(
  table: &str,
  column: &ColumnPlan,
  row_ordinal: u64,
  cell: &Cell,
  values: &mut Vec<Value>,
) -> Result<(), NativeDatabaseError> {
  let unrepresentable = || NativeDatabaseError::UnrepresentableCell {
    table: table.to_owned(),
    column: column.name.clone(),
    row_ordinal,
    candidate: cell.describe(),
    destination: column.kind.sql().to_owned(),
  };
  match (cell, column.kind) {
    (Cell::Null, _) => {
      if !column.nullable {
        return Err(NativeDatabaseError::NullInRequiredColumn {
          table: table.to_owned(),
          column: column.name.clone(),
          row_ordinal,
        });
      }
      if column.kind == NativeColumnKind::TaggedNumeric {
        values.extend([Value::UTinyInt(0), Value::Null, Value::Null]);
      } else {
        values.push(Value::Null);
      }
    }
    (Cell::Integer(value), NativeColumnKind::BigInt) => {
      values.push(Value::BigInt(*value));
    }
    (Cell::Integer(value), NativeColumnKind::TaggedNumeric) => {
      values.extend([Value::UTinyInt(1), Value::BigInt(*value), Value::Null]);
    }
    (Cell::Real(bits), NativeColumnKind::Double) => {
      values.push(Value::Double(f64::from_bits(*bits)));
    }
    (Cell::Real(bits), NativeColumnKind::TaggedNumeric) => {
      values.extend([
        Value::UTinyInt(2),
        Value::Null,
        Value::Double(f64::from_bits(*bits)),
      ]);
    }
    (Cell::Text(value), NativeColumnKind::Varchar) => {
      values.push(Value::Text(value.clone()));
    }
    (Cell::Blob(value), NativeColumnKind::Blob) => {
      values.push(Value::Blob(value.clone()));
    }
    _ => return Err(unrepresentable()),
  }
  Ok(())
}

fn create_staging_sql(staging: &str, plan: &[ColumnPlan]) -> String {
  let mut columns = Vec::new();
  for (index, column) in plan.iter().enumerate() {
    if column.kind == NativeColumnKind::TaggedNumeric {
      columns.push(format!("\"c{index:04}_tag\" UTINYINT NOT NULL"));
      columns.push(format!("\"c{index:04}_i\" BIGINT"));
      columns.push(format!("\"c{index:04}_r\" DOUBLE"));
    } else {
      columns.push(format!("\"c{index:04}\" {}", column.kind.sql()));
    }
  }
  columns.push("\"__hv_stage_ordinal\" UBIGINT NOT NULL".to_owned());
  format!(
    "CREATE TEMP TABLE {} ({})",
    quote_identifier(staging),
    columns.join(", ")
  )
}

fn append_staging(
  destination: &Connection,
  staging: &str,
  plan: &[ColumnPlan],
  rows: usize,
  values: Vec<Value>,
) -> Result<(), NativeDatabaseError> {
  let width = plan
    .iter()
    .map(|column| {
      if column.kind == NativeColumnKind::TaggedNumeric {
        3
      } else {
        1
      }
    })
    .sum::<usize>()
    + 1;
  if values.len() != rows * width {
    return Err(NativeDatabaseError::finalization(
      "stage a finalized page",
      format!("staged {} values for {rows} rows of {width}", values.len()),
    ));
  }
  let mut appender = destination
    .appender_to_catalog_and_db(staging, "temp", "main")
    .map_err(|error| {
      NativeDatabaseError::duckdb("open the finalization staging appender", error)
    })?;
  for row in values.chunks(width) {
    appender
      .append_row(appender_params_from_iter(row.to_vec()))
      .map_err(|error| {
        NativeDatabaseError::duckdb("append a finalized staging row", error)
      })?;
  }
  appender.flush().map_err(|error| {
    NativeDatabaseError::duckdb("flush the finalization staging appender", error)
  })
}

fn insert_from_staging_sql(table: &str, staging: &str, plan: &[ColumnPlan]) -> String {
  let mut names = Vec::with_capacity(plan.len());
  let mut projections = Vec::with_capacity(plan.len());
  for (index, column) in plan.iter().enumerate() {
    names.push(quote_identifier(&column.name));
    if column.kind == NativeColumnKind::TaggedNumeric {
      projections.push(format!(
        "CASE \"c{index:04}_tag\" \
         WHEN 1 THEN union_value(i := \"c{index:04}_i\")::UNION(i BIGINT, r DOUBLE) \
         WHEN 2 THEN union_value(r := \"c{index:04}_r\")::UNION(i BIGINT, r DOUBLE) \
         ELSE NULL END"
      ));
    } else {
      projections.push(format!("\"c{index:04}\""));
    }
  }
  format!(
    "INSERT INTO {} ({}) SELECT {} FROM temp.main.{} ORDER BY \"__hv_stage_ordinal\"",
    quote_identifier(table),
    names.join(", "),
    projections.join(", "),
    quote_identifier(staging)
  )
}

/// Import the SQLite identity state the allocator has to keep reproducing.
///
/// AUTOINCREMENT tables take their high-water mark from the candidate's copied
/// `sqlite_sequence` row, raised to the largest surviving id when the two
/// disagree: a sequence that lags its own rows must never hand out a colliding
/// id. Rowid tables record no high-water mark because SQLite derives their next
/// id from the current maximum.
fn write_identities(
  candidate: &Connection,
  destination: &Connection,
  schema: &NativeSchemaDefinition,
) -> Result<(), NativeDatabaseError> {
  let sequences = read_sqlite_sequences(candidate)?;
  for identity in schema.identities {
    let (mode, high_water) = match identity.mode {
      NativeIdentityMode::RowId => ("rowid", 0_i64),
      NativeIdentityMode::AutoIncrement {
        sqlite_sequence_name,
      } => {
        let sequence = sequences.get(sqlite_sequence_name).copied().unwrap_or(0);
        let maximum: i64 = destination
          .query_row(
            &format!(
              "SELECT COALESCE(MAX({}), 0) FROM {}",
              quote_identifier(identity.column),
              quote_identifier(identity.table)
            ),
            [],
            |row| row.get(0),
          )
          .map_err(|error| {
            NativeDatabaseError::duckdb("read the finalized identity maximum", error)
          })?;
        ("autoincrement", sequence.max(maximum))
      }
    };
    destination
      .execute(
        &format!(
          "INSERT INTO {} VALUES (?, ?, ?, ?)",
          quote_identifier(NATIVE_IDENTITY_TABLE)
        ),
        duckdb::params![identity.table, identity.column, mode, high_water],
      )
      .map_err(|error| {
        NativeDatabaseError::duckdb("write the native identity metadata", error)
      })?;
  }
  Ok(())
}

fn read_sqlite_sequences(
  candidate: &Connection,
) -> Result<BTreeMap<String, i64>, NativeDatabaseError> {
  let present: i64 = candidate
    .query_row(
      "SELECT count(*) FROM information_schema.tables WHERE table_name = ?",
      [SQLITE_SEQUENCE_TABLE],
      |row| row.get(0),
    )
    .map_err(|error| {
      NativeDatabaseError::duckdb("look for the copied sqlite_sequence table", error)
    })?;
  if present == 0 {
    return Ok(BTreeMap::new());
  }
  let mut statement = candidate
    .prepare(&format!(
      "SELECT name, seq FROM {}",
      quote_identifier(SQLITE_SEQUENCE_TABLE)
    ))
    .map_err(|error| {
      NativeDatabaseError::duckdb("prepare the sqlite_sequence read", error)
    })?;
  let rows = statement
    .query_map([], |row| {
      Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })
    .map_err(|error| {
      NativeDatabaseError::duckdb("read the copied sqlite_sequence rows", error)
    })?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|error| {
      NativeDatabaseError::duckdb("read the copied sqlite_sequence rows", error)
    })?;
  Ok(rows.into_iter().collect())
}

fn write_metadata(
  destination: &Connection,
  schema: &NativeSchemaDefinition,
  candidate_path: &Path,
  source_schema_sha256: &str,
  total_rows: u64,
) -> Result<(), NativeDatabaseError> {
  let rows = i64::try_from(total_rows).map_err(|_| {
    NativeDatabaseError::finalization(
      "write the native metadata",
      "total row count does not fit a signed 64-bit value",
    )
  })?;
  destination
    .execute(
      &format!(
        "INSERT INTO {} VALUES (?, ?, ?, ?, ?)",
        quote_identifier(NATIVE_METADATA_TABLE)
      ),
      duckdb::params![
        FINALIZED_UNSELECTED,
        i64::from(schema.version),
        candidate_path.to_string_lossy().as_ref(),
        source_schema_sha256,
        rows
      ],
    )
    .map(|_| ())
    .map_err(|error| NativeDatabaseError::duckdb("write the native metadata", error))
}

/// Reopen the closed file and recompute what was written.
fn verify_finalized(
  database_path: &Path,
  spill: &Path,
  schema: &NativeSchemaDefinition,
  source_schema_sha256: &str,
  copied: &[CopiedTable],
) -> Result<Vec<NativeTableReport>, NativeDatabaseError> {
  let connection = open_database(database_path, AccessMode::ReadOnly, spill)?;
  let (state, version, digest): (String, i64, String) = connection
    .query_row(
      &format!(
        "SELECT state, schema_version, source_schema_sha256 FROM {}",
        quote_identifier(NATIVE_METADATA_TABLE)
      ),
      [],
      |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .map_err(|error| {
      NativeDatabaseError::duckdb("read the reopened native metadata", error)
    })?;
  if state != FINALIZED_UNSELECTED
    || version != i64::from(schema.version)
    || digest != source_schema_sha256
  {
    return Err(NativeDatabaseError::Verification {
      message: "the reopened native metadata does not match what was written".to_owned(),
    });
  }
  let identities: i64 = connection
    .query_row(
      &format!(
        "SELECT count(*) FROM {}",
        quote_identifier(NATIVE_IDENTITY_TABLE)
      ),
      [],
      |row| row.get(0),
    )
    .map_err(|error| {
      NativeDatabaseError::duckdb("read the reopened identity metadata", error)
    })?;
  if identities != schema.identities.len() as i64 {
    return Err(NativeDatabaseError::Verification {
      message: format!(
        "the reopened database records {identities} identities for {} declared tables",
        schema.identities.len()
      ),
    });
  }

  let mut reports = Vec::with_capacity(copied.len());
  for table in copied {
    let columns = read_columns(&connection, &table.name)?;
    let key_columns = read_primary_key(&connection, &table.name)?;
    let mut reader = PagedReader::new(
      &connection,
      &table.name,
      columns
        .iter()
        .map(|column| ReadColumn {
          name: column.name.clone(),
          kind: column.kind,
        })
        .collect(),
      key_columns,
    )?;
    let mut digest = RowMultisetDigest::default();
    while let Some(page) = reader.next_page()? {
      for row in page {
        digest.add_row(&row);
      }
    }
    let reopened_rows = count_rows(&connection, &table.name)?;
    if reopened_rows != table.rows
      || digest.rows() != table.rows
      || digest != table.digest
    {
      return Err(NativeDatabaseError::Verification {
        message: format!(
          "{} read back {reopened_rows} rows with digest {} after reopen, but {} rows with digest {} were written",
          table.name,
          digest.encode(),
          table.rows,
          table.digest.encode()
        ),
      });
    }
    reports.push(NativeTableReport {
      name: table.name.clone(),
      candidate_rows: table.candidate_rows,
      copied_rows: table.rows,
      reopened_rows,
      copied_digest: table.digest.encode(),
      reopened_digest: digest.encode(),
      unconvertible_timestamps: table.unconvertible_timestamps,
    });
  }
  Ok(reports)
}

#[derive(Clone, Debug)]
struct TableColumn {
  name: String,
  kind: NativeColumnKind,
  nullable: bool,
}

fn read_columns(
  connection: &Connection,
  table: &str,
) -> Result<Vec<TableColumn>, NativeDatabaseError> {
  let mut statement = connection
    .prepare(
      "SELECT column_name, data_type, CAST(is_nullable AS VARCHAR) \
       FROM information_schema.columns WHERE table_name = ? ORDER BY ordinal_position",
    )
    .map_err(|error| {
      NativeDatabaseError::duckdb("prepare the table column read", error)
    })?;
  let rows = statement
    .query_map([table], |row| {
      Ok((
        row.get::<_, String>(0)?,
        row.get::<_, String>(1)?,
        row.get::<_, String>(2)?,
      ))
    })
    .map_err(|error| NativeDatabaseError::duckdb("read the table columns", error))?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|error| NativeDatabaseError::duckdb("read the table columns", error))?;
  rows
    .into_iter()
    .map(|(name, data_type, nullable)| {
      let kind = NativeColumnKind::parse(&data_type).ok_or_else(|| {
        NativeDatabaseError::SchemaMismatch {
          table: table.to_owned(),
          detail: format!("column {name} has unsupported storage type {data_type}"),
        }
      })?;
      Ok(TableColumn {
        name,
        kind,
        nullable: nullable.eq_ignore_ascii_case("yes")
          || nullable.eq_ignore_ascii_case("true"),
      })
    })
    .collect()
}

fn read_primary_key(
  connection: &Connection,
  table: &str,
) -> Result<Vec<String>, NativeDatabaseError> {
  let mut statement = connection
    .prepare(
      "SELECT unnest(constraint_column_names) FROM duckdb_constraints() \
       WHERE table_name = ? AND constraint_type = 'PRIMARY KEY'",
    )
    .map_err(|error| {
      NativeDatabaseError::duckdb("prepare the primary key read", error)
    })?;
  let columns = statement
    .query_map([table], |row| row.get::<_, String>(0))
    .map_err(|error| NativeDatabaseError::duckdb("read the primary key", error))?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|error| NativeDatabaseError::duckdb("read the primary key", error))?;
  if columns.is_empty() {
    return Err(NativeDatabaseError::SchemaMismatch {
      table: table.to_owned(),
      detail: "the native schema declares no primary key, so the finalized table \
               cannot be verified in bounded pages"
        .to_owned(),
    });
  }
  Ok(columns)
}

fn count_rows(connection: &Connection, table: &str) -> Result<u64, NativeDatabaseError> {
  let count: i64 = connection
    .query_row(
      &format!("SELECT COUNT(*) FROM {}", quote_identifier(table)),
      [],
      |row| row.get(0),
    )
    .map_err(|error| NativeDatabaseError::duckdb("count a table", error))?;
  u64::try_from(count).map_err(|_| NativeDatabaseError::Verification {
    message: format!("DuckDB returned a negative row count for {table}"),
  })
}

fn read_candidate_provenance(
  candidate: &Connection,
) -> Result<String, NativeDatabaseError> {
  let (kind, digest): (String, Vec<u8>) = candidate
    .query_row(
      &format!(
        "SELECT snapshot_kind, source_schema_digest FROM {}",
        quote_identifier(SNAPSHOT_METADATA_TABLE)
      ),
      [],
      |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .map_err(|error| NativeDatabaseError::CandidateSnapshot {
      message: format!("failed to read {SNAPSHOT_METADATA_TABLE}: {error}"),
    })?;
  if kind != "immutable_source_snapshot" {
    return Err(NativeDatabaseError::CandidateSnapshot {
      message: format!("unexpected snapshot kind {kind}"),
    });
  }
  Ok(encode_hex(&digest))
}

fn open_database(
  path: &Path,
  access_mode: AccessMode,
  spill: &Path,
) -> Result<Connection, NativeDatabaseError> {
  let config = Config::default()
    .access_mode(access_mode)
    .and_then(|config| config.threads(2))
    .and_then(|config| config.max_memory("128MB"))
    .and_then(|config| config.enable_autoload_extension(false))
    .map_err(|error| {
      NativeDatabaseError::duckdb("configure a finalization database", error)
    })?;
  let connection = Connection::open_with_flags(path, config).map_err(|error| {
    NativeDatabaseError::duckdb("open a finalization database", error)
  })?;
  super::configure_spill(&connection, spill)?;
  Ok(connection)
}

fn require_no_wal(database: &Path) -> Result<(), NativeDatabaseError> {
  let mut wal = database.as_os_str().to_os_string();
  wal.push(".wal");
  if PathBuf::from(&wal).exists() {
    return Err(NativeDatabaseError::Verification {
      message: format!(
        "a write-ahead log remains beside the finalized database: {}",
        PathBuf::from(wal).display()
      ),
    });
  }
  Ok(())
}

fn encode_hex(bytes: &[u8]) -> String {
  use std::fmt::Write;

  let mut encoded = String::with_capacity(bytes.len() * 2);
  for byte in bytes {
    write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
  }
  encoded
}
