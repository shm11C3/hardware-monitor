//! What a native archive writer stores for one write cycle's instant.
//!
//! Two things have to be right, and neither may be guessed:
//!
//! - The **stored bytes** are whatever sqlx would have written for the same
//!   `DateTime<Utc>`, because every timestamp comparison in these families is a
//!   byte-wise text comparison ([`sqlite_timestamp_text`]).
//! - The **derived key** is whatever finalization would have computed for those
//!   bytes. It is not `timestamp_millis()`: SQLite's date parser rounds a
//!   fractional second to the nearest millisecond (`(i64)(s*1000 + 0.5)`) while
//!   `timestamp_millis` truncates, so the two disagree on a `.xxx500`
//!   microsecond stamp. Rather than re-deriving the rounding rule in Rust -
//!   where the tie lands on whichever side binary64 rounds `s*1000` to - the
//!   writer runs the same oracle the finalizer runs
//!   ([`super::epoch::EpochMilliseconds`]), so the key is by construction the
//!   value finalization would have produced for the row.
//!
//! The oracle opens a scratch in-memory SQLite database, so [`write_stamp`]
//! runs it on a blocking task once per write cycle - not once per row, and
//! never on the DuckDB lane. [`stamp_for_write`] is the blocking core, shared
//! verbatim with the `DATA_ARCHIVE`/GPU lane so both families derive the key
//! the one way.
//!
//! The key is an `i64`, not an `Option<i64>`: the text being converted is this
//! module's own rendering of a `DateTime<Utc>`, which SQLite's date parser
//! always reads. A NULL back from the oracle would mean the two disagreed
//! about our own output, which is a defect rather than a missing reading, so it
//! is refused instead of silently stored as an unqueryable NULL.

use chrono::{DateTime, Utc};

use super::NativeDatabaseError;
use super::epoch::EpochMilliseconds;
use super::process_stats::sqlite_timestamp_text;

/// One write cycle's timestamp as the stored text and the derived epoch key
/// finalization would have computed from it.
///
/// Blocking: opens an in-memory SQLite database. Call it from
/// [`write_stamp`] rather than directly from an async writer.
pub(super) fn stamp_for_write(
  timestamp: &DateTime<Utc>,
) -> Result<(String, i64), NativeDatabaseError> {
  let text = sqlite_timestamp_text(timestamp);
  let mut oracle = EpochMilliseconds::open()?;
  let epoch_milliseconds = oracle
    .convert(std::slice::from_ref(&Some(text.clone())))?
    .into_iter()
    .next()
    .flatten()
    .ok_or_else(|| {
      NativeDatabaseError::finalization(
        "derive a native timestamp key",
        format!("SQLite did not read the rendered stamp {text:?} as an instant"),
      )
    })?;
  Ok((text, epoch_milliseconds))
}

/// [`stamp_for_write`] off the async runtime's worker threads.
pub(super) async fn write_stamp(
  timestamp: DateTime<Utc>,
) -> Result<(String, i64), NativeDatabaseError> {
  tokio::task::spawn_blocking(move || stamp_for_write(&timestamp))
    .await
    .map_err(|error| NativeDatabaseError::Worker {
      message: error.to_string(),
    })?
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn rounds_a_half_millisecond_stamp_the_way_sqlite_does() {
    // `timestamp_millis()` truncates both of these to ...000; SQLite rounds
    // the second one up. The key must follow SQLite.
    let below: DateTime<Utc> = "2026-09-01T00:00:00.000499Z".parse().unwrap();
    let tie: DateTime<Utc> = "2026-09-01T00:00:00.000500Z".parse().unwrap();
    assert_eq!(below.timestamp_millis(), tie.timestamp_millis());

    let below = write_stamp(below).await.unwrap();
    let tie = write_stamp(tie).await.unwrap();

    assert_eq!(below.0, "2026-09-01T00:00:00.000499+00:00");
    assert_eq!(tie.0, "2026-09-01T00:00:00.000500+00:00");
    assert_eq!(below.1, 1_788_220_800_000);
    assert_eq!(tie.1, 1_788_220_800_001);
  }

  #[tokio::test]
  async fn keeps_a_pre_epoch_stamp_negative() {
    let (text, epoch_milliseconds) =
      write_stamp("1969-12-31T23:59:59.500Z".parse().unwrap())
        .await
        .unwrap();
    assert_eq!(text, "1969-12-31T23:59:59.500+00:00");
    assert_eq!(epoch_milliseconds, -500);
  }
}
