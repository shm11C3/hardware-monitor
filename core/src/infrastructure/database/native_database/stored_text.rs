//! Decoding stored text exactly as the SQLite readers decode it.
//!
//! The SQLite modules never parse these columns themselves: they hand the
//! cell to sqlx, which owns one definition of "what text is a date". A native
//! reader has no sqlx value to hand over, so the same grammar is restated
//! here - deliberately as a transcription of
//! `sqlx_sqlite::types::chrono::decode_datetime_from_text` and its `NaiveDate`
//! sibling rather than as a "better" parser, because a reader that accepted
//! one more spelling than SQLite would return a row the oracle does not.
//!
//! `core/tests/duckdb_ambient_fan.rs` pins the transcription against sqlx
//! itself over every spelling the archives can carry.

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, Offset, TimeZone, Utc};

use super::NativeDatabaseError;
use crate::persistence::cooling_rollup::CpuLoadBand;

/// The patterns sqlx tries after RFC 3339, in its order. Order matters: the
/// first pattern that parses wins, and several of these overlap.
const SQLITE_DATETIME_FORMATS: &[&str] = &[
  "%F %T%.f",
  "%F %R",
  "%F %RZ",
  "%F %R%:z",
  "%F %T%.fZ",
  "%F %T%.f%:z",
  "%FT%R",
  "%FT%RZ",
  "%FT%R%:z",
  "%FT%T%.f",
  "%FT%T%.fZ",
  "%FT%T%.f%:z",
];

fn parse_datetime(value: &str) -> Option<DateTime<FixedOffset>> {
  if let Ok(datetime) = DateTime::parse_from_rfc3339(value) {
    return Some(datetime);
  }
  for format in SQLITE_DATETIME_FORMATS {
    if let Ok(datetime) = DateTime::parse_from_str(value, format) {
      return Some(datetime);
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(value, format) {
      return Some(Utc.fix().from_utc_datetime(&naive));
    }
  }
  None
}

/// The instant the SQLite reader would have decoded from `value`.
pub(super) fn datetime(
  table: &'static str,
  column: &'static str,
  value: &str,
) -> Result<DateTime<Utc>, NativeDatabaseError> {
  parse_datetime(value)
    .map(|datetime| Utc.from_utc_datetime(&datetime.naive_utc()))
    .ok_or_else(|| NativeDatabaseError::UndecodableStoredValue {
      table,
      column,
      kind: "an instant",
      value: value.to_owned(),
    })
}

/// The calendar day the SQLite reader would have decoded from `value`.
pub(super) fn date(
  table: &'static str,
  column: &'static str,
  value: &str,
) -> Result<NaiveDate, NativeDatabaseError> {
  NaiveDate::parse_from_str(value, "%F").map_err(|_| {
    NativeDatabaseError::UndecodableStoredValue {
      table,
      column,
      kind: "a calendar day",
      value: value.to_owned(),
    }
  })
}

/// The CPU-load band a stored `band` key names, refusing an unknown key the
/// way `cooling_covariate_daily_summary`'s `sqlx::Error::Decode` does.
pub(super) fn band(
  table: &'static str,
  value: &str,
) -> Result<CpuLoadBand, NativeDatabaseError> {
  CpuLoadBand::from_column_key(value).ok_or_else(|| {
    NativeDatabaseError::UndecodableStoredValue {
      table,
      column: "band",
      kind: "a CPU-load band",
      value: value.to_owned(),
    }
  })
}

/// The same defensive clamp every SQLite row decoder applies to a count
/// column that is `NOT NULL` and only ever written from a `u32`.
pub(super) fn count(value: i64) -> u32 {
  value.max(0) as u32
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn reads_the_shapes_the_archives_carry_and_refuses_the_rest() {
    let at = |text: &str| datetime("T", "timestamp", text).unwrap();
    assert_eq!(
      at("2026-09-01T00:00:00+00:00"),
      "2026-09-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap()
    );
    assert_eq!(at("2026-09-01T00:00:00Z"), at("2026-09-01 00:00:00"));
    assert_eq!(at("2026-09-01T09:00:00+09:00"), at("2026-09-01T00:00:00Z"));
    assert!(datetime("T", "timestamp", "not a timestamp").is_err());

    assert_eq!(
      date("T", "date", "2026-09-01").unwrap(),
      NaiveDate::from_ymd_opt(2026, 9, 1).unwrap()
    );
    // `%F` is lenient about zero padding, and so is the sqlx reader this
    // transcribes - the point is to accept exactly what SQLite's reader
    // accepts, not to be stricter than it.
    assert_eq!(
      date("T", "date", "2026-9-1").unwrap(),
      NaiveDate::from_ymd_opt(2026, 9, 1).unwrap()
    );
    assert!(date("T", "date", "2026-09").is_err());
    assert!(date("T", "date", "not a day").is_err());
    assert_eq!(band("T", "mid").unwrap(), CpuLoadBand::Mid);
    assert!(band("T", "warm").is_err());
    assert_eq!(count(-3), 0);
  }

  /// The transcription is only worth having if it agrees with the reader it
  /// transcribes, so it is compared against sqlx itself rather than against a
  /// second opinion about what SQLite accepts - including on the spellings
  /// both refuse, where a native reader that accepted one more shape would
  /// return a row the oracle does not.
  #[tokio::test]
  async fn agrees_with_sqlx_on_every_spelling_the_archives_can_carry() {
    use sqlx::{Connection, Row};

    let mut sqlite = sqlx::sqlite::SqliteConnection::connect("sqlite::memory:")
      .await
      .unwrap();

    for text in [
      "2026-09-01T00:00:00+00:00",
      "2026-09-01T00:00:00Z",
      "2026-09-01 00:00:00",
      "2026-09-01 00:00",
      "2026-09-01T00:00",
      "2026-09-01T09:00:00+09:00",
      "2026-09-01T00:00:00.000500+00:00",
      "2026-09-01 00:00:00.123",
      "not a timestamp",
      "",
    ] {
      let oracle: Option<DateTime<Utc>> = sqlx::query("SELECT ?")
        .bind(text)
        .fetch_one(&mut sqlite)
        .await
        .unwrap()
        .try_get(0)
        .ok();
      let ours = datetime("T", "timestamp", text).ok();
      assert_eq!(ours, oracle, "instant from {text:?}");
    }

    for text in ["2026-09-01", "2026-9-1", "2026-09", "not a day", ""] {
      let oracle: Option<NaiveDate> = sqlx::query("SELECT ?")
        .bind(text)
        .fetch_one(&mut sqlite)
        .await
        .unwrap()
        .try_get(0)
        .ok();
      let ours = date("T", "date", text).ok();
      assert_eq!(ours, oracle, "calendar day from {text:?}");
    }
  }
}
