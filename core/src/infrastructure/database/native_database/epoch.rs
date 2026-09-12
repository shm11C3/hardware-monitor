//! Derived epoch-millisecond keys for stored timestamp text.
//!
//! The finalized schema keeps the original timestamp bytes and adds a
//! `__hv_timestamp_epoch_ms` column so native range queries can compare
//! instants instead of relying on every writer having produced the exact same
//! string shape.
//!
//! The conversion is not reimplemented. It runs the production adapter
//! ([`sqlite_epoch_milliseconds_of`]) inside a scratch in-memory SQLite
//! database, so the derived key is by construction the value the current
//! SQLite queries compute for the same text - including the cases where
//! SQLite's `strftime` returns NULL, which stay NULL here rather than becoming
//! a guessed instant. Reimplementing SQLite's date-string grammar in Rust would
//! be a second, drifting definition of the same fact - and a measured one:
//! [`super::write_stamp`] records the instant where the obvious Rust formula
//! and this adapter disagree. Native writers therefore stamp new rows through
//! the same oracle rather than deriving a key from the instant they hold.

use sqlx::sqlite::SqliteConnection;
use sqlx::{Connection, Row};

use super::NativeDatabaseError;
use crate::infrastructure::database::archive_queries::sqlite_epoch_milliseconds_of;

/// Converts stored timestamp text in bounded batches through one scratch
/// SQLite connection.
pub(super) struct EpochMilliseconds {
  runtime: tokio::runtime::Runtime,
  connection: SqliteConnection,
}

impl EpochMilliseconds {
  pub(super) fn open() -> Result<Self, NativeDatabaseError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
      .enable_all()
      .build()
      .map_err(|error| NativeDatabaseError::Worker {
        message: format!("failed to start the finalization runtime: {error}"),
      })?;
    let connection = runtime
      .block_on(SqliteConnection::connect("sqlite::memory:"))
      .map_err(|error| {
        NativeDatabaseError::finalization(
          "open the SQLite timestamp conversion oracle",
          error,
        )
      })?;
    Ok(Self {
      runtime,
      connection,
    })
  }

  /// Convert one batch of stored texts. `None` inputs (a NULL timestamp cell)
  /// and texts SQLite cannot read as an instant both yield `None`.
  pub(super) fn convert(
    &mut self,
    texts: &[Option<String>],
  ) -> Result<Vec<Option<i64>>, NativeDatabaseError> {
    let mut converted = vec![None; texts.len()];
    let present = texts
      .iter()
      .enumerate()
      .filter_map(|(index, text)| text.as_ref().map(|text| (index, text)))
      .collect::<Vec<_>>();
    if present.is_empty() {
      return Ok(converted);
    }

    let values = std::iter::repeat_n("(?,?)", present.len())
      .collect::<Vec<_>>()
      .join(",");
    let sql = format!(
      "WITH t(i, s) AS (VALUES {values}) SELECT i, {} FROM t",
      sqlite_epoch_milliseconds_of("s")
    );
    let mut query = sqlx::query(&sql);
    for (index, text) in &present {
      query = query.bind(*index as i64).bind(*text);
    }
    let rows = self
      .runtime
      .block_on(query.fetch_all(&mut self.connection))
      .map_err(|error| {
        NativeDatabaseError::finalization("convert stored timestamp text", error)
      })?;
    if rows.len() != present.len() {
      return Err(NativeDatabaseError::finalization(
        "convert stored timestamp text",
        format!(
          "expected {} converted rows, got {}",
          present.len(),
          rows.len()
        ),
      ));
    }
    for row in rows {
      let index: i64 = row.try_get(0).map_err(|error| {
        NativeDatabaseError::finalization("convert stored timestamp text", error)
      })?;
      let milliseconds: Option<i64> = row.try_get(1).map_err(|error| {
        NativeDatabaseError::finalization(
          "convert stored timestamp text",
          format!("SQLite returned a non-integer epoch value: {error}"),
        )
      })?;
      let index = usize::try_from(index).map_err(|_| {
        NativeDatabaseError::finalization(
          "convert stored timestamp text",
          "conversion batch index went negative",
        )
      })?;
      *converted.get_mut(index).ok_or_else(|| {
        NativeDatabaseError::finalization(
          "convert stored timestamp text",
          "conversion batch index is out of range",
        )
      })? = milliseconds;
    }
    Ok(converted)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn reproduces_sqlite_for_every_spelling_including_the_ones_it_refuses() {
    let mut adapter = EpochMilliseconds::open().unwrap();
    let texts = vec![
      // The shape sqlx stores for a `DateTime<Utc>`.
      Some("2026-09-01T00:00:00+00:00".to_owned()),
      Some("2026-09-01T00:00:00.125+00:00".to_owned()),
      // Fixture and legacy spellings SQLite also accepts.
      Some("2026-09-01T00:00:00Z".to_owned()),
      Some("2026-09-01 00:00:00".to_owned()),
      Some("2026-09-01T09:00:00+09:00".to_owned()),
      Some("1969-12-31T23:59:59.500Z".to_owned()),
      // SQLite reads neither of these as an instant.
      Some("not a timestamp".to_owned()),
      Some(String::new()),
      None,
    ];

    let converted = adapter.convert(&texts).unwrap();

    assert_eq!(converted[0], Some(1_788_220_800_000));
    assert_eq!(converted[1], Some(1_788_220_800_125));
    assert_eq!(converted[2], converted[0]);
    assert_eq!(converted[3], converted[0]);
    assert_eq!(converted[4], converted[0]);
    assert_eq!(converted[5], Some(-500));
    assert_eq!(converted[6], None);
    assert_eq!(converted[7], None);
    assert_eq!(converted[8], None);
  }

  #[test]
  fn keeps_batch_positions_when_only_some_inputs_convert() {
    let mut adapter = EpochMilliseconds::open().unwrap();
    let converted = adapter
      .convert(&[
        None,
        Some("2026-09-01T00:00:00Z".to_owned()),
        Some("still not a timestamp".to_owned()),
        Some("2026-09-01T00:00:01Z".to_owned()),
      ])
      .unwrap();
    assert_eq!(
      converted,
      vec![None, Some(1_788_220_800_000), None, Some(1_788_220_801_000)]
    );
  }
}
