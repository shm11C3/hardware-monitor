//! Durable authority selection, and what to conclude from the files left on
//! disk.
//!
//! Selecting a backend is the one step of the conversion that cannot be undone
//! by deleting a file: afterwards the native database, not SQLite, holds the
//! rows the application has written. It therefore has to survive a crash at any
//! point, and - more importantly - a crash must never leave a state where two
//! files both look authoritative.
//!
//! # The order, and the single repairable gap
//!
//! The record is written twice: once inside the native database's own metadata
//! (`state = 'selected'`, committed, checkpointed and synced) and once in a
//! small marker file beside it. The native database is written **first**. The
//! only state a crash can leave between the two writes is therefore "the
//! database says selected, the marker is missing", and that state is
//! repairable without guessing: the database that says `selected` is the one
//! that was chosen, and the marker is rewritten from it
//! ([`repair_authority_marker`]). The reverse order would leave a marker
//! claiming a selection the database never recorded, which is not repairable -
//! the marker alone cannot say whether the transaction committed.
//!
//! Every other disagreement between the two is reported rather than repaired.
//! Recovery code that guesses which of two files is authoritative is how
//! history gets lost silently, and the numbers needed to decide are in the
//! report.
//!
//! # What is deliberately not here
//!
//! Retiring the SQLite source once a later startup has verified the selection
//! is App lifecycle work, and whether that source is renamed in place or copied
//! aside is still an open maintainer question ([`super::preflight`] budgets no
//! second copy of it). Nothing in this module removes or renames the source.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use duckdb::{AccessMode, Connection};
use serde::{Deserialize, Serialize};

use super::NativeDatabaseError;
use super::cell::quote_identifier;
use super::finalize::{
  FINALIZED_UNSELECTED, NATIVE_METADATA_TABLE, SELECTED, open_database, require_no_wal,
};
use super::reconcile::NativeReconciliationReport;

/// The marker file name, resolved by the caller against the directory that
/// holds the databases.
pub const AUTHORITY_MARKER_FILE_NAME: &str = "hv-database.authority.json";

/// The prefix every conversion work directory shares, so leftover debris is
/// recognizable without knowing which step produced it.
const WORK_PREFIX: &str = ".hardwarevisualizer-duckdb-";

const MARKER_VERSION: u32 = 1;

/// Proof that a reconciliation caught this file up to its source and read it
/// back afterwards.
///
/// It has no public constructor: the only way to obtain one is
/// [`super::reconcile_native_database`], so "select whatever is lying there"
/// cannot be expressed, and neither can "select the file finalization just
/// produced" - a finalized file copies one snapshot taken while the
/// application kept writing, so it is stale by construction.
///
/// # Precondition the caller owns
///
/// Reconciliation makes the native database equal the source *at the moment it
/// captured its candidate*. Rows written to SQLite after that are not in the
/// file, and nothing here can see them. The App lifecycle owner must therefore
/// quiesce every SQLite writer before the final reconciliation and keep them
/// quiesced until [`select_native_database`] returns; selecting after a
/// reconciliation that ran against a live writer silently drops whatever was
/// written in between.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedNativeDatabase {
  path: PathBuf,
  schema_version: u32,
  source_schema_sha256: String,
  total_rows: u64,
}

impl VerifiedNativeDatabase {
  /// Only [`super::reconcile`] may mint proof, and only from a report it has
  /// just verified against the reopened file.
  pub(super) fn from_reconciliation(report: &NativeReconciliationReport) -> Self {
    Self {
      path: report.native_database_path.clone(),
      schema_version: report.schema_version,
      source_schema_sha256: report.source_schema_sha256.clone(),
      total_rows: report.total_rows,
    }
  }

  pub fn path(&self) -> &Path {
    &self.path
  }

  pub fn schema_version(&self) -> u32 {
    self.schema_version
  }

  pub fn source_schema_sha256(&self) -> &str {
    &self.source_schema_sha256
  }

  pub fn total_rows(&self) -> u64 {
    self.total_rows
  }
}

/// The on-disk marker. Deliberately small: every field is one the native
/// database records too, so the two can be compared rather than trusted.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthorityMarker {
  pub version: u32,
  /// The file name only. An absolute path would be wrong the moment the
  /// application data directory moves, and the directory is the caller's.
  pub native_database_file_name: String,
  pub schema_version: u32,
  pub source_schema_sha256: String,
  pub total_rows: u64,
}

/// The two databases, the marker beside them, and the directory conversion
/// debris would appear in.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorityPaths {
  pub source_database: PathBuf,
  pub native_database: PathBuf,
  pub marker: PathBuf,
}

impl AuthorityPaths {
  /// The conventional layout: both databases and the marker in one directory.
  pub fn in_directory(
    directory: &Path,
    source_file_name: &str,
    native_file_name: &str,
  ) -> Self {
    Self {
      source_database: directory.join(source_file_name),
      native_database: directory.join(native_file_name),
      marker: directory.join(AUTHORITY_MARKER_FILE_NAME),
    }
  }
}

/// The two states a native database's metadata may record. Anything else is
/// treated as unreadable rather than mapped onto one of them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeState {
  FinalizedUnselected,
  Selected,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MarkerFacts {
  Absent,
  /// Present but unusable: unreadable bytes, invalid JSON, or a version this
  /// build does not know.
  Unreadable,
  Present(AuthorityMarker),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeMetadataFacts {
  /// No native database file at all.
  Absent,
  /// The file exists but its metadata could not be read as a finalized native
  /// database.
  Unreadable,
  Present {
    state: NativeState,
    schema_version: u32,
    source_schema_sha256: String,
    /// The row count the file records, compared against the marker's so a
    /// database restored from a different backup than its marker is reported
    /// rather than trusted.
    source_rows: u64,
  },
}

/// Everything [`inspect_authority`] is allowed to look at, gathered by
/// [`observe_authority`]. Separating the two keeps the decision a pure function
/// that a test can drive through every state without a filesystem.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorityFacts {
  pub source_database_present: bool,
  pub native_database_present: bool,
  pub native_database_file_name: String,
  /// A `.wal` beside the native database: a file that was not checkpointed and
  /// closed cleanly.
  pub native_write_ahead_log_present: bool,
  /// A `.hardwarevisualizer-duckdb-*` directory: an interrupted conversion.
  pub work_directory_present: bool,
  pub marker: MarkerFacts,
  pub native_metadata: NativeMetadataFacts,
  pub expected_schema_version: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthorityInconsistency {
  MarkerUnreadable,
  MarkerWithoutNativeDatabase,
  MarkerNamesAnotherDatabase,
  NativeMetadataUnreadable,
  /// The marker claims a selection the database did not record.
  MarkerAheadOfNativeState,
  /// A selected database built for a schema version this build does not run -
  /// the downgrade case.
  SchemaVersionMismatch,
  MarkerDisagreesWithNativeDatabase,
  /// The repairable gap: the selection committed, the marker did not land.
  SelectedWithoutMarker,
  /// Native files exist but the SQLite source they were built from is gone.
  SourceDatabaseMissing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthorityRecovery {
  /// Report the numbers and change nothing. Guessing here loses history.
  StopAndReport,
  /// Rewrite the marker from the native database's own committed metadata.
  RepairSelectionMarkerFromNativeMetadata,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthorityState {
  /// The ordinary state before any conversion, and after a fresh install.
  SqliteAuthoritative,
  /// Debris from an interrupted conversion. `resumable` means a complete
  /// finalized file is present, so the conversion resumes at reconciliation;
  /// otherwise the debris is discarded and the copy restarts.
  ConversionInProgress {
    resumable: bool,
  },
  /// A verified native database exists, and SQLite is still authoritative.
  FinalizedUnselected,
  NativeSelected,
  Inconsistent {
    reason: AuthorityInconsistency,
    recovery: AuthorityRecovery,
  },
}

/// Record `native_database` as the authoritative backend.
///
/// Opens the database itself, so no [`super::NativeDatabase`] owner may be live
/// on the same file: DuckDB refuses a second instance, and on Windows the file
/// could not be synced afterwards either.
///
/// The database's own metadata is committed, checkpointed and synced before the
/// marker is written, so the only interruption window leaves the one state
/// [`repair_authority_marker`] can close.
pub async fn select_native_database(
  paths: AuthorityPaths,
  verified: VerifiedNativeDatabase,
) -> Result<AuthorityMarker, NativeDatabaseError> {
  tokio::task::spawn_blocking(move || select(&paths, &verified))
    .await
    .map_err(|error| NativeDatabaseError::Worker {
      message: error.to_string(),
    })?
}

fn select(
  paths: &AuthorityPaths,
  verified: &VerifiedNativeDatabase,
) -> Result<AuthorityMarker, NativeDatabaseError> {
  if paths.native_database != verified.path {
    return Err(NativeDatabaseError::UnverifiedSelection {
      detail: format!(
        "the verified conversion is {} but {} is being selected",
        verified.path.display(),
        paths.native_database.display()
      ),
    });
  }
  let spill = selection_spill()?;

  // Scoped so every DuckDB handle on the file is released before the file is
  // synced and the marker is written: Windows refuses to reopen, rename or
  // delete a file another instance still holds.
  {
    let connection =
      open_database(&paths.native_database, AccessMode::ReadWrite, spill.path())?;
    let (state, schema_version, source_schema_sha256, source_rows, reconciled) =
      read_metadata_row(&connection)?;
    if state != FINALIZED_UNSELECTED {
      return Err(NativeDatabaseError::UnexpectedState {
        operation: "selected",
        state,
        expected: FINALIZED_UNSELECTED,
      });
    }
    // The file says for itself whether a reconciliation committed into it, so
    // a proof that happens to describe a look-alike file - one re-finalized
    // from the same unchanged source, say - still cannot select it.
    if !reconciled {
      return Err(NativeDatabaseError::UnverifiedSelection {
        detail: format!(
          "{} records no committed reconciliation, so it holds one snapshot \
           taken while the source was still being written",
          paths.native_database.display()
        ),
      });
    }
    let schema_version = u32::try_from(schema_version).unwrap_or(u32::MAX);
    let source_rows = u64::try_from(source_rows).unwrap_or(u64::MAX);
    if schema_version != verified.schema_version
      || source_schema_sha256 != verified.source_schema_sha256
      || source_rows != verified.total_rows
    {
      return Err(NativeDatabaseError::UnverifiedSelection {
        detail: format!(
          "the file records schema version {schema_version}, source schema \
           {source_schema_sha256} and {source_rows} rows; the verified \
           conversion recorded {}, {} and {}",
          verified.schema_version, verified.source_schema_sha256, verified.total_rows
        ),
      });
    }

    connection
      .execute_batch(&format!(
        "BEGIN TRANSACTION; UPDATE {} SET state = '{SELECTED}'; COMMIT",
        quote_identifier(NATIVE_METADATA_TABLE)
      ))
      .map_err(|error| {
        NativeDatabaseError::duckdb("record the native selection", error)
      })?;
    connection.execute_batch("CHECKPOINT").map_err(|error| {
      NativeDatabaseError::duckdb("checkpoint the selected database", error)
    })?;
  }
  require_no_wal(&paths.native_database)?;
  sync_file(&paths.native_database)?;

  let marker = AuthorityMarker {
    version: MARKER_VERSION,
    native_database_file_name: file_name_of(&paths.native_database)?,
    schema_version: verified.schema_version,
    source_schema_sha256: verified.source_schema_sha256.clone(),
    total_rows: verified.total_rows,
  };
  write_marker_atomically(&paths.marker, &marker)?;
  Ok(marker)
}

/// Close the one repairable gap: a database that committed `selected` while its
/// marker never landed.
///
/// Refuses anything else, including a database that is merely finalized: the
/// marker is a record of a decision, never the decision itself.
///
/// Opens the database, so it carries the same "no live owner" precondition as
/// [`observe_authority`].
pub fn repair_authority_marker(
  paths: &AuthorityPaths,
) -> Result<AuthorityMarker, NativeDatabaseError> {
  let spill = selection_spill()?;
  // Scoped so the database is closed before the marker is renamed over.
  let marker = {
    let connection =
      open_database(&paths.native_database, AccessMode::ReadOnly, spill.path())?;
    let (state, schema_version, source_schema_sha256, source_rows, _) =
      read_metadata_row(&connection)?;
    if state != SELECTED {
      return Err(NativeDatabaseError::UnexpectedState {
        operation: "repaired into a selection marker",
        state,
        expected: SELECTED,
      });
    }
    AuthorityMarker {
      version: MARKER_VERSION,
      native_database_file_name: file_name_of(&paths.native_database)?,
      schema_version: u32::try_from(schema_version).unwrap_or(u32::MAX),
      source_schema_sha256,
      total_rows: u64::try_from(source_rows).unwrap_or(u64::MAX),
    }
  };
  write_marker_atomically(&paths.marker, &marker)?;
  Ok(marker)
}

/// The metadata row, as `(state, schema_version, source_schema_sha256,
/// source_rows, reconciled)`.
/// Selection reads and updates one metadata row, so its spill never holds
/// anything. It goes in the system temporary directory rather than beside the
/// databases: a directory named with the conversion work prefix, left behind by
/// an interrupted selection, would read as interrupted *conversion* debris to
/// [`inspect_authority`].
fn selection_spill() -> Result<tempfile::TempDir, NativeDatabaseError> {
  tempfile::Builder::new()
    .prefix(WORK_PREFIX)
    .tempdir()
    .map_err(|error| {
      NativeDatabaseError::selection("reserve the selection spill directory", error)
    })
}

fn read_metadata_row(
  connection: &Connection,
) -> Result<(String, i64, String, i64, bool), NativeDatabaseError> {
  connection
    .query_row(
      &format!(
        "SELECT state, schema_version, source_schema_sha256, source_rows, \
         reconciled FROM {}",
        quote_identifier(NATIVE_METADATA_TABLE)
      ),
      [],
      |row| {
        Ok((
          row.get(0)?,
          row.get(1)?,
          row.get(2)?,
          row.get(3)?,
          row.get(4)?,
        ))
      },
    )
    .map_err(|error| {
      NativeDatabaseError::duckdb("read the native metadata to select", error)
    })
}

fn file_name_of(path: &Path) -> Result<String, NativeDatabaseError> {
  path
    .file_name()
    .map(|name| name.to_string_lossy().into_owned())
    .ok_or_else(|| {
      NativeDatabaseError::selection(
        "name the selected database",
        "the native database path has no file name",
      )
    })
}

/// Write to a temporary file, sync it, rename it over the marker, then sync the
/// directory: a reader sees either the old marker or the whole new one.
fn write_marker_atomically(
  marker_path: &Path,
  marker: &AuthorityMarker,
) -> Result<(), NativeDatabaseError> {
  let directory = marker_path
    .parent()
    .filter(|path| !path.as_os_str().is_empty())
    .ok_or_else(|| {
      NativeDatabaseError::selection(
        "resolve the selection marker directory",
        "the marker must have a parent directory",
      )
    })?;
  let encoded = serde_json::to_vec_pretty(marker).map_err(|error| {
    NativeDatabaseError::selection("encode the selection marker", error)
  })?;
  let mut temporary = tempfile::Builder::new()
    .prefix(".hv-database.authority.")
    .suffix(".json")
    .tempfile_in(directory)
    .map_err(|error| {
      NativeDatabaseError::selection("create the selection marker", error)
    })?;
  temporary.write_all(&encoded).map_err(|error| {
    NativeDatabaseError::selection("write the selection marker", error)
  })?;
  temporary.as_file().sync_all().map_err(|error| {
    NativeDatabaseError::selection("sync the selection marker", error)
  })?;
  temporary.persist(marker_path).map_err(|error| {
    NativeDatabaseError::selection("publish the selection marker", error.error)
  })?;
  sync_directory(directory)
}

fn sync_file(path: &Path) -> Result<(), NativeDatabaseError> {
  OpenOptions::new()
    .read(true)
    .write(true)
    .open(path)
    .and_then(|file| file.sync_all())
    .map_err(|error| NativeDatabaseError::selection("sync the selected database", error))
}

/// Renames are only durable once the directory entry itself is synced. Windows
/// has no directory handle to sync, and its rename is already ordered, so the
/// call is skipped rather than faked.
fn sync_directory(directory: &Path) -> Result<(), NativeDatabaseError> {
  match File::open(directory).and_then(|handle| handle.sync_all()) {
    Ok(()) => Ok(()),
    Err(_) if cfg!(windows) => Ok(()),
    Err(error) => Err(NativeDatabaseError::selection(
      "sync the selection marker directory",
      error,
    )),
  }
}

/// Read everything on disk that the authority decision depends on.
///
/// Never writes, and never fails: an unreadable marker or database is a fact
/// about the state, not an error, and [`inspect_authority`] has an arm for it.
///
/// # Call this before the backend is opened, never beside it
///
/// Reading the native metadata means opening the file as a second DuckDB
/// instance, and DuckDB refuses a file another instance already holds. Called
/// while a [`super::NativeDatabase`] owner is live, this would therefore report
/// the metadata as *unreadable* - which [`inspect_authority`] turns into
/// `ConversionInProgress`, an alarming answer about a perfectly healthy
/// database. It belongs at startup, before any owner is opened, and after one
/// has been closed.
pub fn observe_authority(
  paths: &AuthorityPaths,
  expected_schema_version: u32,
) -> AuthorityFacts {
  let native_database_present = paths.native_database.is_file();
  let mut write_ahead_log = paths.native_database.as_os_str().to_os_string();
  write_ahead_log.push(".wal");

  AuthorityFacts {
    source_database_present: paths.source_database.is_file(),
    native_database_present,
    native_database_file_name: paths
      .native_database
      .file_name()
      .map(|name| name.to_string_lossy().into_owned())
      .unwrap_or_default(),
    native_write_ahead_log_present: PathBuf::from(write_ahead_log).is_file(),
    work_directory_present: work_directory_present(&paths.native_database),
    marker: observe_marker(&paths.marker),
    native_metadata: if native_database_present {
      observe_native_metadata(&paths.native_database)
    } else {
      NativeMetadataFacts::Absent
    },
    expected_schema_version,
  }
}

fn work_directory_present(native_database: &Path) -> bool {
  let Some(directory) = native_database.parent() else {
    return false;
  };
  let Ok(entries) = fs::read_dir(directory) else {
    return false;
  };
  entries.filter_map(Result::ok).any(|entry| {
    entry.file_name().to_string_lossy().starts_with(WORK_PREFIX)
      && entry.file_type().is_ok_and(|kind| kind.is_dir())
  })
}

fn observe_marker(path: &Path) -> MarkerFacts {
  match fs::read(path) {
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => MarkerFacts::Absent,
    Err(_) => MarkerFacts::Unreadable,
    Ok(bytes) => match serde_json::from_slice::<AuthorityMarker>(&bytes) {
      Ok(marker) if marker.version == MARKER_VERSION => MarkerFacts::Present(marker),
      _ => MarkerFacts::Unreadable,
    },
  }
}

fn observe_native_metadata(path: &Path) -> NativeMetadataFacts {
  let Ok(spill) = selection_spill() else {
    return NativeMetadataFacts::Unreadable;
  };
  let Ok(connection) = open_database(path, AccessMode::ReadOnly, spill.path()) else {
    return NativeMetadataFacts::Unreadable;
  };
  let Ok((state, schema_version, source_schema_sha256, source_rows, _)) =
    read_metadata_row(&connection)
  else {
    return NativeMetadataFacts::Unreadable;
  };
  let state = match state.as_str() {
    FINALIZED_UNSELECTED => NativeState::FinalizedUnselected,
    SELECTED => NativeState::Selected,
    _ => return NativeMetadataFacts::Unreadable,
  };
  let Ok(schema_version) = u32::try_from(schema_version) else {
    return NativeMetadataFacts::Unreadable;
  };
  let Ok(source_rows) = u64::try_from(source_rows) else {
    return NativeMetadataFacts::Unreadable;
  };
  NativeMetadataFacts::Present {
    state,
    schema_version,
    source_schema_sha256,
    source_rows,
  }
}

/// Decide what the observed files mean. Pure: the same facts always give the
/// same answer, and no arm creates, empties or reverts a database.
pub fn inspect_authority(facts: &AuthorityFacts) -> AuthorityState {
  let stop = |reason| AuthorityState::Inconsistent {
    reason,
    recovery: AuthorityRecovery::StopAndReport,
  };

  match &facts.marker {
    MarkerFacts::Unreadable => stop(AuthorityInconsistency::MarkerUnreadable),
    MarkerFacts::Present(marker) => {
      if !facts.native_database_present {
        return stop(AuthorityInconsistency::MarkerWithoutNativeDatabase);
      }
      if marker.native_database_file_name != facts.native_database_file_name {
        return stop(AuthorityInconsistency::MarkerNamesAnotherDatabase);
      }
      let NativeMetadataFacts::Present {
        state,
        schema_version,
        source_schema_sha256,
        source_rows,
      } = &facts.native_metadata
      else {
        return stop(AuthorityInconsistency::NativeMetadataUnreadable);
      };
      if *state == NativeState::FinalizedUnselected {
        return stop(AuthorityInconsistency::MarkerAheadOfNativeState);
      }
      if *schema_version != facts.expected_schema_version {
        return stop(AuthorityInconsistency::SchemaVersionMismatch);
      }
      // Every field the marker carries is one the database records too, so all
      // of them are compared. A restore that put back a database and a marker
      // from different backups agrees on the file name and the schema but not
      // on how many rows were selected.
      if marker.schema_version != *schema_version
        || &marker.source_schema_sha256 != source_schema_sha256
        || marker.total_rows != *source_rows
      {
        return stop(AuthorityInconsistency::MarkerDisagreesWithNativeDatabase);
      }
      AuthorityState::NativeSelected
    }
    MarkerFacts::Absent => {
      if matches!(
        facts.native_metadata,
        NativeMetadataFacts::Present {
          state: NativeState::Selected,
          ..
        }
      ) {
        return AuthorityState::Inconsistent {
          reason: AuthorityInconsistency::SelectedWithoutMarker,
          recovery: AuthorityRecovery::RepairSelectionMarkerFromNativeMetadata,
        };
      }
      if !facts.source_database_present
        && (facts.native_database_present || facts.work_directory_present)
      {
        return stop(AuthorityInconsistency::SourceDatabaseMissing);
      }
      match &facts.native_metadata {
        NativeMetadataFacts::Unreadable => {
          AuthorityState::ConversionInProgress { resumable: false }
        }
        NativeMetadataFacts::Present { .. } => {
          // Finalized and unselected. A write-ahead log or leftover work
          // directory means the conversion was interrupted after the file was
          // complete, so it resumes at reconciliation rather than recopying.
          if facts.native_write_ahead_log_present || facts.work_directory_present {
            AuthorityState::ConversionInProgress { resumable: true }
          } else {
            AuthorityState::FinalizedUnselected
          }
        }
        NativeMetadataFacts::Absent => {
          if facts.work_directory_present {
            AuthorityState::ConversionInProgress { resumable: false }
          } else {
            AuthorityState::SqliteAuthoritative
          }
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn marker() -> AuthorityMarker {
    AuthorityMarker {
      version: MARKER_VERSION,
      native_database_file_name: "hv-database.duckdb".to_owned(),
      schema_version: 1,
      source_schema_sha256: "abc".to_owned(),
      total_rows: 10,
    }
  }

  fn facts() -> AuthorityFacts {
    AuthorityFacts {
      source_database_present: true,
      native_database_present: true,
      native_database_file_name: "hv-database.duckdb".to_owned(),
      native_write_ahead_log_present: false,
      work_directory_present: false,
      marker: MarkerFacts::Present(marker()),
      native_metadata: NativeMetadataFacts::Present {
        state: NativeState::Selected,
        schema_version: 1,
        source_schema_sha256: "abc".to_owned(),
        source_rows: 10,
      },
      expected_schema_version: 1,
    }
  }

  fn inconsistent(reason: AuthorityInconsistency) -> AuthorityState {
    AuthorityState::Inconsistent {
      reason,
      recovery: AuthorityRecovery::StopAndReport,
    }
  }

  #[test]
  fn a_marker_and_an_agreeing_selected_database_is_the_only_selected_state() {
    assert_eq!(inspect_authority(&facts()), AuthorityState::NativeSelected);
  }

  #[test]
  fn a_fresh_install_and_a_plain_sqlite_installation_stay_on_sqlite() {
    let mut observed = facts();
    observed.marker = MarkerFacts::Absent;
    observed.native_database_present = false;
    observed.native_metadata = NativeMetadataFacts::Absent;
    assert_eq!(
      inspect_authority(&observed),
      AuthorityState::SqliteAuthoritative
    );

    observed.source_database_present = false;
    assert_eq!(
      inspect_authority(&observed),
      AuthorityState::SqliteAuthoritative
    );
  }

  #[test]
  fn a_finalized_database_alone_leaves_sqlite_authoritative() {
    let mut observed = facts();
    observed.marker = MarkerFacts::Absent;
    observed.native_metadata = NativeMetadataFacts::Present {
      state: NativeState::FinalizedUnselected,
      schema_version: 1,
      source_schema_sha256: "abc".to_owned(),
      source_rows: 10,
    };
    assert_eq!(
      inspect_authority(&observed),
      AuthorityState::FinalizedUnselected
    );

    // Debris beside a complete file means the conversion stopped after
    // finalization, so it resumes at reconciliation.
    observed.work_directory_present = true;
    assert_eq!(
      inspect_authority(&observed),
      AuthorityState::ConversionInProgress { resumable: true }
    );
    observed.work_directory_present = false;
    observed.native_write_ahead_log_present = true;
    assert_eq!(
      inspect_authority(&observed),
      AuthorityState::ConversionInProgress { resumable: true }
    );
  }

  #[test]
  fn an_incomplete_native_file_or_bare_work_directory_restarts_the_copy() {
    let mut observed = facts();
    observed.marker = MarkerFacts::Absent;
    observed.native_metadata = NativeMetadataFacts::Unreadable;
    assert_eq!(
      inspect_authority(&observed),
      AuthorityState::ConversionInProgress { resumable: false }
    );

    observed.native_database_present = false;
    observed.native_metadata = NativeMetadataFacts::Absent;
    observed.work_directory_present = true;
    assert_eq!(
      inspect_authority(&observed),
      AuthorityState::ConversionInProgress { resumable: false }
    );
  }

  #[test]
  fn a_committed_selection_without_its_marker_is_the_one_repairable_state() {
    let mut observed = facts();
    observed.marker = MarkerFacts::Absent;
    assert_eq!(
      inspect_authority(&observed),
      AuthorityState::Inconsistent {
        reason: AuthorityInconsistency::SelectedWithoutMarker,
        recovery: AuthorityRecovery::RepairSelectionMarkerFromNativeMetadata,
      }
    );
  }

  #[test]
  fn every_other_disagreement_stops_rather_than_guessing() {
    let mut observed = facts();
    observed.marker = MarkerFacts::Unreadable;
    assert_eq!(
      inspect_authority(&observed),
      inconsistent(AuthorityInconsistency::MarkerUnreadable)
    );

    let mut observed = facts();
    observed.native_database_present = false;
    observed.native_metadata = NativeMetadataFacts::Absent;
    assert_eq!(
      inspect_authority(&observed),
      inconsistent(AuthorityInconsistency::MarkerWithoutNativeDatabase)
    );

    let mut observed = facts();
    observed.native_database_file_name = "other.duckdb".to_owned();
    assert_eq!(
      inspect_authority(&observed),
      inconsistent(AuthorityInconsistency::MarkerNamesAnotherDatabase)
    );

    let mut observed = facts();
    observed.native_metadata = NativeMetadataFacts::Unreadable;
    assert_eq!(
      inspect_authority(&observed),
      inconsistent(AuthorityInconsistency::NativeMetadataUnreadable)
    );

    let mut observed = facts();
    observed.native_metadata = NativeMetadataFacts::Present {
      state: NativeState::FinalizedUnselected,
      schema_version: 1,
      source_schema_sha256: "abc".to_owned(),
      source_rows: 10,
    };
    assert_eq!(
      inspect_authority(&observed),
      inconsistent(AuthorityInconsistency::MarkerAheadOfNativeState)
    );

    // The downgrade case: a selected database this build cannot run.
    let mut observed = facts();
    observed.expected_schema_version = 2;
    assert_eq!(
      inspect_authority(&observed),
      inconsistent(AuthorityInconsistency::SchemaVersionMismatch)
    );

    let mut observed = facts();
    observed.native_metadata = NativeMetadataFacts::Present {
      state: NativeState::Selected,
      schema_version: 1,
      source_schema_sha256: "different".to_owned(),
      source_rows: 10,
    };
    assert_eq!(
      inspect_authority(&observed),
      inconsistent(AuthorityInconsistency::MarkerDisagreesWithNativeDatabase)
    );

    // A restore that put back a database and a marker from different backups:
    // everything agrees except how many rows were selected.
    let mut observed = facts();
    observed.native_metadata = NativeMetadataFacts::Present {
      state: NativeState::Selected,
      schema_version: 1,
      source_schema_sha256: "abc".to_owned(),
      source_rows: 11,
    };
    assert_eq!(
      inspect_authority(&observed),
      inconsistent(AuthorityInconsistency::MarkerDisagreesWithNativeDatabase)
    );

    let mut observed = facts();
    observed.marker = MarkerFacts::Absent;
    observed.source_database_present = false;
    observed.native_metadata = NativeMetadataFacts::Present {
      state: NativeState::FinalizedUnselected,
      schema_version: 1,
      source_schema_sha256: "abc".to_owned(),
      source_rows: 10,
    };
    assert_eq!(
      inspect_authority(&observed),
      inconsistent(AuthorityInconsistency::SourceDatabaseMissing)
    );
  }

  #[test]
  fn the_marker_round_trips_and_an_unknown_version_is_unreadable() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(AUTHORITY_MARKER_FILE_NAME);
    write_marker_atomically(&path, &marker()).unwrap();
    assert_eq!(observe_marker(&path), MarkerFacts::Present(marker()));

    std::fs::write(&path, br#"{"version":99}"#).unwrap();
    assert_eq!(observe_marker(&path), MarkerFacts::Unreadable);
    std::fs::write(&path, b"not json").unwrap();
    assert_eq!(observe_marker(&path), MarkerFacts::Unreadable);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(observe_marker(&path), MarkerFacts::Absent);
  }
}
