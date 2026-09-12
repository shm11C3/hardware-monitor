//! The dedicated blocking owner of one finalized native database.
//!
//! duckdb-rs is synchronous, so connections are never handed out. Two owned
//! threads - one read lane, one write lane over `try_clone`d connections to the
//! same instance - execute closures sent through bounded channels, so a slow
//! reader can neither block a commit nor let callers queue unbounded work.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use duckdb::{AccessMode, Config, Connection, OptionalExt, Transaction, params};
use tempfile::TempDir;
use tokio::sync::{Mutex as AsyncMutex, RwLock, mpsc, oneshot};

use super::NativeDatabaseError;
use super::cell::quote_identifier;
use super::finalize::{
  FINALIZED_UNSELECTED, NATIVE_IDENTITY_TABLE, NATIVE_METADATA_TABLE, SELECTED,
};

#[derive(Clone, Copy, Debug)]
pub struct NativeDatabaseOptions {
  pub expected_schema_version: u32,
  /// How many requests may wait per lane before a caller is made to wait.
  pub request_capacity: usize,
}

impl NativeDatabaseOptions {
  pub fn new(expected_schema_version: u32) -> Self {
    Self {
      expected_schema_version,
      request_capacity: 32,
    }
  }
}

/// A one-shot cancellation token for exactly one request.
///
/// Claimed when it is attached to a request so two requests can never share an
/// interrupt handle, which would let one caller's cancellation abort another's
/// statement.
#[derive(Clone, Debug)]
pub struct NativeCancellation {
  inner: Arc<CancellationState>,
}

struct CancellationState {
  claimed: AtomicBool,
  cancelled: AtomicBool,
  interrupt: Mutex<Option<Arc<duckdb::InterruptHandle>>>,
}

impl std::fmt::Debug for CancellationState {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter
      .debug_struct("CancellationState")
      .field("claimed", &self.claimed)
      .field("cancelled", &self.cancelled)
      .finish_non_exhaustive()
  }
}

impl NativeCancellation {
  pub fn new() -> Self {
    Self {
      inner: Arc::new(CancellationState {
        claimed: AtomicBool::new(false),
        cancelled: AtomicBool::new(false),
        interrupt: Mutex::new(None),
      }),
    }
  }

  pub fn cancel(&self) {
    self.inner.cancelled.store(true, Ordering::Release);
    let interrupt = self
      .inner
      .interrupt
      .lock()
      .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(interrupt) = interrupt.as_ref() {
      interrupt.interrupt();
    }
  }

  pub fn is_cancelled(&self) -> bool {
    self.inner.cancelled.load(Ordering::Acquire)
  }

  fn claim(&self) -> Result<(), NativeDatabaseError> {
    self
      .inner
      .claimed
      .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
      .map(|_| ())
      .map_err(|_| NativeDatabaseError::CancellationAlreadyUsed)
  }
}

impl Default for NativeCancellation {
  fn default() -> Self {
    Self::new()
  }
}

pub struct NativeConnectionContext<'a> {
  connection: &'a mut Connection,
  cancellation: NativeCancellation,
  healthy: bool,
}

impl NativeConnectionContext<'_> {
  pub fn connection(&mut self) -> &mut Connection {
    self.connection
  }

  pub fn check_cancelled(&self) -> Result<(), NativeDatabaseError> {
    if self.cancellation.is_cancelled() {
      Err(NativeDatabaseError::Cancelled)
    } else {
      Ok(())
    }
  }

  /// Run `operation` inside one DuckDB transaction, committing only if it
  /// succeeded and was not cancelled.
  ///
  /// A rollback that itself fails marks the lane unhealthy and stops it: a
  /// connection whose transaction state is unknown must not serve the next
  /// request.
  pub fn with_transaction<T, F>(&mut self, operation: F) -> Result<T, NativeDatabaseError>
  where
    F: FnOnce(&NativeTransactionContext<'_, '_>) -> Result<T, NativeDatabaseError>,
  {
    self.check_cancelled()?;
    let transaction = self
      .connection
      .transaction()
      .map_err(|error| NativeDatabaseError::duckdb("begin transaction", error))?;
    let context = NativeTransactionContext {
      transaction: &transaction,
      cancellation: &self.cancellation,
    };
    let result = operation(&context);
    let cancelled = self.cancellation.is_cancelled();
    match result {
      Ok(value) if !cancelled => transaction
        .commit()
        .map(|_| value)
        .map_err(|error| NativeDatabaseError::duckdb("commit transaction", error)),
      result => {
        if let Err(rollback_error) = transaction.rollback() {
          self.healthy = false;
          return Err(NativeDatabaseError::Worker {
            message: format!("failed to roll back native transaction: {rollback_error}"),
          });
        }
        if cancelled {
          Err(NativeDatabaseError::Cancelled)
        } else {
          result
        }
      }
    }
  }
}

pub struct NativeTransactionContext<'transaction, 'connection> {
  transaction: &'transaction Transaction<'connection>,
  cancellation: &'transaction NativeCancellation,
}

impl NativeTransactionContext<'_, '_> {
  pub fn connection(&self) -> &Connection {
    self.transaction
  }

  pub fn check_cancelled(&self) -> Result<(), NativeDatabaseError> {
    if self.cancellation.is_cancelled() {
      Err(NativeDatabaseError::Cancelled)
    } else {
      Ok(())
    }
  }

  /// Allocate the next `id` for `table`.
  ///
  /// **Identity contract.** These ids are record identities, not domain
  /// identities: a Process row is identified by its recorded `(pid,
  /// process_name)` tuple, a Storage Health record by its producer-supplied
  /// `storage:hmac-sha256:v1:...` device key, and an archived GPU row by its
  /// opaque `gpu_id`. Nothing joins on the values produced here, so the only
  /// contract they owe is the one SQLite gave them, which finalization recorded
  /// per table in `__hv_native_identities`:
  ///
  /// - `autoincrement` reproduces `INTEGER PRIMARY KEY AUTOINCREMENT`. The next
  ///   id is the stored high-water mark plus one, and the mark advances in the
  ///   same transaction as the insert. Deleting the highest row therefore does
  ///   **not** release its id, and a rolled-back insert releases it exactly as
  ///   SQLite's `sqlite_sequence` update would.
  /// - `rowid` reproduces plain `INTEGER PRIMARY KEY`. The next id is the
  ///   current maximum plus one, so deleting the highest row does release its
  ///   id - SQLite's behavior, deliberately preserved rather than "fixed",
  ///   because an archive migrated to a stricter rule would start handing out
  ///   ids the source would not have.
  ///
  /// Both modes read and write inside the caller's transaction, so two
  /// concurrent writers cannot be handed the same id: the write lane is the
  /// only lane that commits.
  pub fn next_id(&self, table: &str) -> Result<i64, NativeDatabaseError> {
    self.check_cancelled()?;
    let metadata: Option<(String, String, i64)> = self
      .transaction
      .query_row(
        &format!(
          "SELECT column_name, mode, high_water FROM {NATIVE_IDENTITY_TABLE} WHERE table_name = ?"
        ),
        [table],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
      )
      .optional()
      .map_err(|error| {
        NativeDatabaseError::duckdb("read native identity metadata", error)
      })?;
    let Some((column, mode, high_water)) = metadata else {
      return Err(NativeDatabaseError::finalization(
        "allocate a native id",
        format!("table {table} has no native identity definition"),
      ));
    };
    let next = match mode.as_str() {
      "rowid" => {
        let sql = format!(
          "SELECT COALESCE(MAX({}), 0) FROM {}",
          quote_identifier(&column),
          quote_identifier(table)
        );
        let maximum: i64 = self
          .transaction
          .query_row(&sql, [], |row| row.get(0))
          .map_err(|error| {
            NativeDatabaseError::duckdb("read native rowid maximum", error)
          })?;
        maximum.checked_add(1)
      }
      "autoincrement" => high_water.checked_add(1),
      other => {
        return Err(NativeDatabaseError::finalization(
          "allocate a native id",
          format!("table {table} has unsupported identity mode {other}"),
        ));
      }
    }
    .ok_or_else(|| {
      NativeDatabaseError::finalization(
        "allocate a native id",
        format!("native identity for {table} exhausted signed 64-bit ids"),
      )
    })?;
    if mode == "autoincrement" {
      self
        .transaction
        .execute(
          &format!(
            "UPDATE {NATIVE_IDENTITY_TABLE} SET high_water = ? WHERE table_name = ?"
          ),
          params![next, table],
        )
        .map_err(|error| NativeDatabaseError::duckdb("advance native identity", error))?;
    }
    Ok(next)
  }
}

type Operation = Box<dyn FnOnce(&mut NativeConnectionContext<'_>) + Send + 'static>;

enum LaneMessage {
  Run {
    cancellation: NativeCancellation,
    operation: Operation,
  },
  Shutdown,
}

struct NativeDatabaseInner {
  read_sender: mpsc::Sender<LaneMessage>,
  write_sender: mpsc::Sender<LaneMessage>,
  lifecycle: RwLock<()>,
  close_lock: AsyncMutex<()>,
  closed: AtomicBool,
  joins: AsyncMutex<Option<(JoinHandle<()>, JoinHandle<()>)>>,
}

#[derive(Clone)]
pub struct NativeDatabase {
  inner: Arc<NativeDatabaseInner>,
}

impl std::fmt::Debug for NativeDatabase {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter
      .debug_struct("NativeDatabase")
      .field("closed", &self.inner.closed.load(Ordering::Acquire))
      .finish_non_exhaustive()
  }
}

impl NativeDatabase {
  /// Open a finalized, unselected native database.
  ///
  /// Refuses a file that finalization never produced, and one whose recorded
  /// schema version is not the version the caller was built against.
  pub async fn open(
    path: impl AsRef<Path>,
    options: NativeDatabaseOptions,
  ) -> Result<Self, NativeDatabaseError> {
    if options.request_capacity == 0 {
      return Err(NativeDatabaseError::InvalidRequestCapacity);
    }
    let path = path.as_ref().to_owned();
    let metadata = std::fs::metadata(&path)
      .map_err(|_| NativeDatabaseError::Unavailable { path: path.clone() })?;
    if !metadata.is_file() {
      return Err(NativeDatabaseError::Unavailable { path });
    }
    let expected_version = options.expected_schema_version;
    let (writer, reader, spill) =
      tokio::task::spawn_blocking(move || open_connections(&path, expected_version))
        .await
        .map_err(|error| NativeDatabaseError::Worker {
          message: error.to_string(),
        })??;
    let spill = Arc::new(spill);
    let (read_sender, read_receiver) = mpsc::channel(options.request_capacity);
    let (write_sender, write_receiver) = mpsc::channel(options.request_capacity);
    let read_spill = Arc::clone(&spill);
    let read_join = std::thread::Builder::new()
      .name("hardviz-duckdb-read".to_owned())
      .spawn(move || run_lane(read_spill, reader, read_receiver))
      .map_err(|error| NativeDatabaseError::Worker {
        message: format!("failed to start native read owner: {error}"),
      })?;
    let write_join = std::thread::Builder::new()
      .name("hardviz-duckdb-write".to_owned())
      .spawn(move || run_lane(spill, writer, write_receiver))
      .map_err(|error| NativeDatabaseError::Worker {
        message: format!("failed to start native write owner: {error}"),
      })?;
    Ok(Self {
      inner: Arc::new(NativeDatabaseInner {
        read_sender,
        write_sender,
        lifecycle: RwLock::new(()),
        close_lock: AsyncMutex::new(()),
        closed: AtomicBool::new(false),
        joins: AsyncMutex::new(Some((read_join, write_join))),
      }),
    })
  }

  /// Stop both lanes and wait for them. Idempotent; every later request fails
  /// with [`NativeDatabaseError::Closed`].
  pub async fn close(&self) -> Result<(), NativeDatabaseError> {
    let _close = self.inner.close_lock.lock().await;
    let mut joins = self.inner.joins.lock().await;
    let Some((read_join, write_join)) = joins.take() else {
      return Ok(());
    };
    {
      let _lifecycle = self.inner.lifecycle.write().await;
      self.inner.closed.store(true, Ordering::Release);
      let _ = self.inner.read_sender.send(LaneMessage::Shutdown).await;
      let _ = self.inner.write_sender.send(LaneMessage::Shutdown).await;
    }
    tokio::task::spawn_blocking(move || {
      read_join.join().map_err(|_| NativeDatabaseError::Worker {
        message: "native read owner panicked".to_owned(),
      })?;
      write_join.join().map_err(|_| NativeDatabaseError::Worker {
        message: "native write owner panicked".to_owned(),
      })?;
      Ok(())
    })
    .await
    .map_err(|error| NativeDatabaseError::Worker {
      message: error.to_string(),
    })?
  }

  pub async fn request_read<T, F>(
    &self,
    cancellation: NativeCancellation,
    operation: F,
  ) -> Result<T, NativeDatabaseError>
  where
    T: Send + 'static,
    F: FnOnce(&mut NativeConnectionContext<'_>) -> Result<T, NativeDatabaseError>
      + Send
      + 'static,
  {
    self
      .request_on_lane(&self.inner.read_sender, cancellation, operation)
      .await
  }

  pub async fn request_write<T, F>(
    &self,
    cancellation: NativeCancellation,
    operation: F,
  ) -> Result<T, NativeDatabaseError>
  where
    T: Send + 'static,
    F: FnOnce(&mut NativeConnectionContext<'_>) -> Result<T, NativeDatabaseError>
      + Send
      + 'static,
  {
    self
      .request_on_lane(&self.inner.write_sender, cancellation, operation)
      .await
  }

  async fn request_on_lane<T, F>(
    &self,
    sender: &mpsc::Sender<LaneMessage>,
    cancellation: NativeCancellation,
    operation: F,
  ) -> Result<T, NativeDatabaseError>
  where
    T: Send + 'static,
    F: FnOnce(&mut NativeConnectionContext<'_>) -> Result<T, NativeDatabaseError>
      + Send
      + 'static,
  {
    cancellation.claim()?;
    let (result_sender, result_receiver) = oneshot::channel();
    let request_cancellation = cancellation.clone();
    let operation = Box::new(move |context: &mut NativeConnectionContext<'_>| {
      let result = context.check_cancelled().and_then(|_| operation(context));
      // A DuckDB interrupt surfaces as an ordinary statement error; the token
      // is what says the error was asked for.
      let result = if request_cancellation.is_cancelled() && result.is_err() {
        Err(NativeDatabaseError::Cancelled)
      } else {
        result
      };
      let _ = result_sender.send(result);
    });
    {
      let _lifecycle = self.inner.lifecycle.read().await;
      if self.inner.closed.load(Ordering::Acquire) {
        return Err(NativeDatabaseError::Closed);
      }
      sender
        .send(LaneMessage::Run {
          cancellation,
          operation,
        })
        .await
        .map_err(|_| NativeDatabaseError::Closed)?;
    }
    result_receiver
      .await
      .map_err(|_| NativeDatabaseError::Worker {
        message: "native database owner ended before returning a request".to_owned(),
      })?
  }
}

fn open_connections(
  path: &Path,
  expected_version: u32,
) -> Result<(Connection, Connection, TempDir), NativeDatabaseError> {
  let parent = path
    .parent()
    .filter(|parent| !parent.as_os_str().is_empty())
    .ok_or_else(|| NativeDatabaseError::Unavailable {
      path: path.to_owned(),
    })?;
  let spill = tempfile::Builder::new()
    .prefix(".hardwarevisualizer-duckdb-runtime-")
    .tempdir_in(parent)
    .map_err(|error| NativeDatabaseError::Worker {
      message: format!("failed to create native spill directory: {error}"),
    })?;
  let config = native_config()?;
  let writer = Connection::open_with_flags(path, config)
    .map_err(|error| NativeDatabaseError::duckdb("open native database", error))?;
  super::configure_spill(&writer, spill.path())?;
  validate_native_metadata(&writer, expected_version)?;
  let reader = writer
    .try_clone()
    .map_err(|error| NativeDatabaseError::duckdb("open native read connection", error))?;
  Ok((writer, reader, spill))
}

fn run_lane(
  _spill: Arc<TempDir>,
  mut connection: Connection,
  mut receiver: mpsc::Receiver<LaneMessage>,
) {
  let interrupt = connection.interrupt_handle();
  while let Some(message) = receiver.blocking_recv() {
    let LaneMessage::Run {
      cancellation,
      operation,
    } = message
    else {
      break;
    };
    {
      let mut active = cancellation
        .inner
        .interrupt
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
      if !cancellation.is_cancelled() {
        *active = Some(Arc::clone(&interrupt));
      }
    }
    let mut context = NativeConnectionContext {
      connection: &mut connection,
      cancellation: cancellation.clone(),
      healthy: true,
    };
    operation(&mut context);
    let healthy = context.healthy;
    *cancellation
      .inner
      .interrupt
      .lock()
      .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    if !healthy {
      break;
    }
  }
}

fn native_config() -> Result<Config, NativeDatabaseError> {
  Config::default()
    .access_mode(AccessMode::ReadWrite)
    .and_then(|config| config.threads(2))
    .and_then(|config| config.max_memory("128MB"))
    .and_then(|config| config.enable_autoload_extension(false))
    .map_err(|error| NativeDatabaseError::duckdb("configure native database", error))
}

fn validate_native_metadata(
  connection: &Connection,
  expected_version: u32,
) -> Result<(), NativeDatabaseError> {
  let present: i64 = connection
    .query_row(
      "SELECT count(*) FROM information_schema.tables WHERE table_name = ?",
      [NATIVE_METADATA_TABLE],
      |row| row.get(0),
    )
    .map_err(|error| {
      NativeDatabaseError::duckdb("look for the native schema metadata", error)
    })?;
  if present == 0 {
    return Err(NativeDatabaseError::Unfinalized);
  }
  let row: Option<(String, i64)> = connection
    .query_row(
      &format!("SELECT state, schema_version FROM {NATIVE_METADATA_TABLE}"),
      [],
      |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(|error| {
      NativeDatabaseError::duckdb("read the native schema metadata", error)
    })?;
  let Some((state, actual)) = row else {
    return Err(NativeDatabaseError::Unfinalized);
  };
  // A selected database is the same finalized file with its authority
  // recorded, so the owner serves it on the same terms. Refusing it here would
  // make the backend unopenable exactly once it became authoritative.
  if state != FINALIZED_UNSELECTED && state != SELECTED {
    return Err(NativeDatabaseError::Unfinalized);
  }
  let actual = u32::try_from(actual).map_err(|_| NativeDatabaseError::Unfinalized)?;
  if actual != expected_version {
    return Err(NativeDatabaseError::IncompatibleSchema {
      expected: expected_version,
      actual,
    });
  }
  Ok(())
}
