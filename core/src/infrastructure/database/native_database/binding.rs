//! What a native writer may bind into a column SQLite would have written.
//!
//! SQLite has no NaN. `sqlite3VdbeMemSetDouble` stores a bound IEEE NaN as
//! NULL, so every SQLite writer in this codebase silently turns a NaN reading
//! into a gap - and the readers above them are built on that: `AVG` skips it,
//! `MIN`/`MAX` ignore it, `COUNT(column)` does not count it, and a lane draws a
//! break rather than a line. DuckDB has no such rule: a bound NaN is a
//! perfectly good DOUBLE, and an archive written natively would start answering
//! NaN where the same rows written through SQLite answer "no reading".
//!
//! That is a difference in *meaning*, not in formatting, so it is removed at
//! the only place it can be: the bind. Measured, not assumed - SQLite's two
//! behaviours are pinned by `binds_a_nan_the_way_sqlite_does` below, against
//! sqlx itself:
//!
//! - into a nullable REAL column, a bound NaN reads back as `typeof() = 'null'`;
//! - into a `NOT NULL` REAL column, the insert *fails* with
//!   `SQLITE_CONSTRAINT_NOTNULL` rather than storing anything.
//!
//! Only NaN is special. `±Infinity` is stored as a real value by both engines
//! and is deliberately left alone: it is an out-of-range reading, not an
//! absent one, and turning it into a gap would be this module inventing a rule
//! SQLite does not have.

use super::NativeDatabaseError;

/// The value a native writer binds where a SQLite writer would have bound
/// `value` - `None` for NaN, because that is what SQLite stores.
///
/// Shared verbatim with the `DATA_ARCHIVE`/GPU lane so both families have one
/// definition of "what SQLite does with a NaN".
pub(super) fn sqlite_real_binding(value: f64) -> Option<f64> {
  if value.is_nan() { None } else { Some(value) }
}

/// [`sqlite_real_binding`] for a column the stable schema declares `NOT NULL`.
///
/// SQLite refuses the row outright there, so a native writer must refuse it
/// too - and must refuse it *before* the statement runs, so the caller is told
/// which column carried the NaN instead of reading it out of a DuckDB
/// constraint message. Storing the NaN instead would be the one outcome
/// neither engine produces.
pub(super) fn required_real(
  table: &'static str,
  column: &'static str,
  value: f64,
) -> Result<f64, NativeDatabaseError> {
  sqlite_real_binding(value)
    .ok_or(NativeDatabaseError::NotANumberInRequiredColumn { table, column })
}

#[cfg(test)]
mod tests {
  use super::*;
  use sqlx::{Connection, Row};

  #[test]
  fn only_nan_becomes_absent() {
    assert_eq!(sqlite_real_binding(0.0), Some(0.0));
    assert_eq!(sqlite_real_binding(-0.0), Some(-0.0));
    assert_eq!(sqlite_real_binding(f64::INFINITY), Some(f64::INFINITY));
    assert_eq!(
      sqlite_real_binding(f64::NEG_INFINITY),
      Some(f64::NEG_INFINITY)
    );
    assert_eq!(sqlite_real_binding(f64::NAN), None);
    // A signalling NaN and a NaN that arrived by widening an `f32` are still
    // NaN, and every reading in these families arrives as an `f32`.
    assert_eq!(sqlite_real_binding(f64::from(f32::NAN)), None);
    assert_eq!(sqlite_real_binding(-f64::NAN), None);

    assert_eq!(required_real("T", "c", 1.5).unwrap(), 1.5);
    assert!(matches!(
      required_real("T", "c", f64::NAN),
      Err(NativeDatabaseError::NotANumberInRequiredColumn {
        table: "T",
        column: "c"
      })
    ));
  }

  /// The two SQLite behaviours this module exists to reproduce, measured
  /// against sqlx rather than taken from the documentation.
  #[tokio::test]
  async fn binds_a_nan_the_way_sqlite_does() {
    let mut sqlite = sqlx::sqlite::SqliteConnection::connect("sqlite::memory:")
      .await
      .unwrap();
    sqlx::query("CREATE TABLE t (optional REAL, required REAL NOT NULL)")
      .execute(&mut sqlite)
      .await
      .unwrap();

    // A NaN in a nullable REAL column is stored as NULL, not as a NaN.
    sqlx::query("INSERT INTO t (optional, required) VALUES (?, 1.0)")
      .bind(f64::NAN)
      .execute(&mut sqlite)
      .await
      .unwrap();
    let row = sqlx::query("SELECT typeof(optional), COUNT(optional) FROM t")
      .fetch_one(&mut sqlite)
      .await
      .unwrap();
    assert_eq!(row.get::<String, _>(0), "null");
    assert_eq!(row.get::<i64, _>(1), 0, "AVG and friends skip it");

    // A NaN in a NOT NULL REAL column fails the insert.
    let refused = sqlx::query("INSERT INTO t (optional, required) VALUES (1.0, ?)")
      .bind(f64::NAN)
      .execute(&mut sqlite)
      .await;
    assert!(
      matches!(&refused, Err(sqlx::Error::Database(error))
        if error.message().contains("NOT NULL constraint failed")),
      "{refused:?}"
    );

    // Infinity is a value in both engines, which is why it is left alone.
    sqlx::query("INSERT INTO t (optional, required) VALUES (?, 1.0)")
      .bind(f64::INFINITY)
      .execute(&mut sqlite)
      .await
      .unwrap();
    let row = sqlx::query("SELECT typeof(optional) FROM t ORDER BY rowid DESC LIMIT 1")
      .fetch_one(&mut sqlite)
      .await
      .unwrap();
    assert_eq!(row.get::<String, _>(0), "real");
  }
}
