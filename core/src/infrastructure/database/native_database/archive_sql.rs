//! SQL fragments that restate the SQLite archive rules against the finalized
//! native schema.
//!
//! Three systematic differences, and nothing else:
//!
//! - **Epoch keys.** SQLite computes `strftime('%s', timestamp)` per row.
//!   Finalization already ran that same adapter over every stored text and
//!   stored the result in `__hv_timestamp_epoch_ms`, so the native queries read
//!   the column instead of recomputing it. A text SQLite cannot read as an
//!   instant left the column NULL, which drops the row from a range predicate
//!   exactly as the NULL from `strftime` does.
//! - **Integer division.** SQLite's `/` between integers truncates toward zero.
//!   DuckDB's `/` returns a DOUBLE, and `//` is the truncating integer
//!   division, so every minute key and bucket floor uses `//`.
//! - **Tagged numeric columns.** `CAST(cpu_avg AS REAL)` reads a column that is
//!   `UNION(i BIGINT, r DOUBLE)` natively, so the union member is extracted and
//!   the integer member converted, which is what SQLite's CAST does.
//!
//! Bucket widths and epoch bounds are `i64` and are written into the statement
//! text; only caller-supplied strings are bound as parameters.

use chrono::NaiveDate;

use crate::infrastructure::database::archive_queries::ArchiveBucketTimestamp;

pub(super) const DATA_EPOCH_MS: &str = "DATA_ARCHIVE.__hv_timestamp_epoch_ms";
pub(super) const AMBIENT_EPOCH_MS: &str = "AMBIENT_ARCHIVE.__hv_timestamp_epoch_ms";
pub(super) const FAN_EPOCH_MS: &str = "FAN_ARCHIVE.__hv_timestamp_epoch_ms";

/// `cooling_daily_summary::sqlite_minute_key` over a native epoch expression.
pub(super) fn minute_key(epoch_milliseconds: &str) -> String {
  format!("(({epoch_milliseconds}) // 60000)")
}

/// `archive_queries::bucket_of_epoch_sql` over a native epoch expression.
pub(super) fn bucket_of_epoch(
  bucket_timestamp: ArchiveBucketTimestamp,
  epoch_milliseconds: &str,
  width: i64,
) -> String {
  match bucket_timestamp {
    ArchiveBucketTimestamp::Start => {
      format!("(({epoch_milliseconds}) // {width}) * {width}")
    }
    ArchiveBucketTimestamp::End => {
      format!("((({epoch_milliseconds}) + {width} - 1) // {width}) * {width}")
    }
  }
}

/// `CAST(<column> AS REAL)` against a `UNION(i BIGINT, r DOUBLE)` column.
pub(super) fn union_real(column: &str) -> String {
  format!(
    "CASE union_tag({column}) \
     WHEN 'i' THEN CAST(union_extract({column}, 'i') AS DOUBLE) \
     ELSE union_extract({column}, 'r') END"
  )
}

/// `strftime('%Y-%m-%dT%H:%M:%S', <text>, '<offset> minutes')` rebuilt from the
/// derived epoch key.
///
/// The key already *is* what SQLite's date parser made of the stored text, so
/// shifting it and formatting the result reproduces the modifier without
/// parsing the text a second time. A NULL key formats to NULL, which is what
/// `strftime` returns for a text it cannot read - so the bracket it guards
/// excludes the row either way.
pub(super) fn shifted_seconds_text(epoch_milliseconds: &str, offset_ms: i64) -> String {
  format!(
    "strftime(make_timestamp(CAST(({epoch_milliseconds}) + {offset_ms} AS BIGINT) * 1000), \
     '%Y-%m-%dT%H:%M:%S')"
  )
}

/// `cooling_daily_summary::preserving_delete_sql` with DuckDB's positional
/// parameters. Pinned against the SQLite builder by
/// `matches_the_sqlite_preserving_delete_rule`.
pub(super) fn preserving_delete_sql(
  table: &str,
  column: &str,
  window_count: usize,
) -> String {
  let mut sql = format!("DELETE FROM {table} WHERE {column} < ?");
  for _ in 0..window_count {
    sql.push_str(&format!(" AND NOT ({column} >= ? AND {column} <= ?)"));
  }
  sql
}

/// The local-date retention cutoff every cooling projection deletes against.
pub(super) fn local_retention_cutoff(retention_days: u32) -> String {
  (chrono::Local::now().date_naive() - chrono::Duration::days(i64::from(retention_days)))
    .format("%Y-%m-%d")
    .to_string()
}

/// The stored spelling of a calendar day, as every cooling writer formats it.
pub(super) fn date_key(date: NaiveDate) -> String {
  date.format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::infrastructure::database::cooling_daily_summary;

  /// The native builder differs from the SQLite one only in how a parameter is
  /// spelled. Restating the clause shape would let the two drift apart
  /// silently, so the shapes are compared instead.
  #[test]
  fn matches_the_sqlite_preserving_delete_rule() {
    for windows in 0..3 {
      let sqlite = cooling_daily_summary::preserving_delete_sql("t", "c", windows);
      let mut expected = sqlite.clone();
      for index in (1..=windows * 2 + 1).rev() {
        expected = expected.replace(&format!("${index}"), "?");
      }
      assert_eq!(preserving_delete_sql("t", "c", windows), expected);
    }
  }

  #[test]
  fn integer_division_is_spelled_the_way_duckdb_truncates() {
    assert_eq!(minute_key("e"), "((e) // 60000)");
    assert_eq!(
      bucket_of_epoch(ArchiveBucketTimestamp::Start, "e", 60000),
      "((e) // 60000) * 60000"
    );
    assert_eq!(
      bucket_of_epoch(ArchiveBucketTimestamp::End, "e", 60000),
      "(((e) + 60000 - 1) // 60000) * 60000"
    );
  }

  /// The reason every minute key and bucket floor is spelled `//`.
  ///
  /// SQLite's `/` between integers truncates toward zero; DuckDB's `/` is a
  /// DOUBLE division and its `//` is the truncating one. The two agree on
  /// every positive epoch, which is why the difference is invisible on a
  /// fixture of present-day rows - so it is pinned here instead, over the
  /// pre-epoch values where `//` and a floor division part company.
  #[tokio::test]
  async fn duckdb_integer_division_truncates_toward_zero_as_sqlite_does() {
    use sqlx::{Connection, Row};

    let cases: [i64; 6] = [-90_001, -60_000, -59_999, -1, 0, 90_001];
    let duckdb = duckdb::Connection::open_in_memory().unwrap();
    let mut sqlite = sqlx::sqlite::SqliteConnection::connect("sqlite::memory:")
      .await
      .unwrap();

    for value in cases {
      let native: i64 = duckdb
        .query_row(
          &format!("SELECT {}", minute_key(&value.to_string())),
          [],
          |row| row.get(0),
        )
        .unwrap();
      let oracle: i64 = sqlx::query(&format!("SELECT ({value} / 60000)"))
        .fetch_one(&mut sqlite)
        .await
        .unwrap()
        .get(0);
      assert_eq!(native, oracle, "minute key of {value}");
    }
    // And it is genuinely different from a floor division, so the choice is
    // load-bearing rather than incidental.
    assert_eq!((-59_999i64) / 60_000, 0);
    assert_eq!((-59_999i64).div_euclid(60_000), -1);
  }
}
