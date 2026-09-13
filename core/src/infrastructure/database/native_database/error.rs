use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum NativeDatabaseError {
  #[error("native database file does not exist or is not a file: {path}")]
  Unavailable { path: PathBuf },
  #[error("candidate database does not exist or is not a file: {path}")]
  CandidateUnavailable { path: PathBuf },
  #[error("native database destination already exists: {path}")]
  DestinationExists { path: PathBuf },
  #[error("native database request capacity must be greater than zero")]
  InvalidRequestCapacity,
  #[error("native database is not finalized")]
  Unfinalized,
  #[error(
    "native database schema version {actual} is incompatible with expected version {expected}"
  )]
  IncompatibleSchema { expected: u32, actual: u32 },
  #[error("native database request cancellation may be attached to only one request")]
  CancellationAlreadyUsed,
  #[error("native database request was cancelled")]
  Cancelled,
  #[error("native database is closed")]
  Closed,
  #[error("native database worker failed: {message}")]
  Worker { message: String },
  #[error("native database operation failed during {context}: {source}")]
  DuckDb {
    context: &'static str,
    #[source]
    source: duckdb::Error,
  },
  #[error("candidate snapshot cannot be finalized: {message}")]
  CandidateSnapshot { message: String },
  #[error("candidate table {table} does not match the supplied native schema: {detail}")]
  SchemaMismatch { table: String, detail: String },
  #[error(
    "candidate cell at {table}.{column}, source row ordinal {row_ordinal}, is {candidate} and the native column is {destination}"
  )]
  UnrepresentableCell {
    table: String,
    column: String,
    row_ordinal: u64,
    candidate: &'static str,
    destination: String,
  },
  #[error(
    "{table}.{column} is NOT NULL and the reading is NaN, which SQLite refuses to store"
  )]
  NotANumberInRequiredColumn {
    table: &'static str,
    column: &'static str,
  },
  #[error(
    "NULL in required native column {table}.{column} at source row ordinal {row_ordinal}"
  )]
  NullInRequiredColumn {
    table: String,
    column: String,
    row_ordinal: u64,
  },
  #[error("finalized native database failed verification: {message}")]
  Verification { message: String },
  #[error(
    "the exact sum of {column} for Process ({pid}, {process_name:?}) exceeds a signed 64-bit integer, where SQLite's average is no longer exact"
  )]
  IntegerSumOverflow {
    column: &'static str,
    pid: i64,
    process_name: String,
  },
  #[error(
    "stored {kind} in {table}.{column} is not a value the SQLite reader decodes: {value:?}"
  )]
  UndecodableStoredValue {
    table: &'static str,
    column: &'static str,
    kind: &'static str,
    value: String,
  },
  #[error("native archive series request is not answerable: {source}")]
  ArchiveSeries {
    #[source]
    source: crate::infrastructure::database::archive_queries::ArchiveSeriesError,
  },
  #[error("native database finalization failed during {context}: {message}")]
  Finalization { context: String, message: String },
}

impl NativeDatabaseError {
  pub(crate) fn duckdb(context: &'static str, source: duckdb::Error) -> Self {
    Self::DuckDb { context, source }
  }

  pub(crate) fn finalization(
    context: impl Into<String>,
    message: impl std::fmt::Display,
  ) -> Self {
    Self::Finalization {
      context: context.into(),
      message: message.to_string(),
    }
  }
}
