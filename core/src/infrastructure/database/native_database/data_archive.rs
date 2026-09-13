//! The DATA_ARCHIVE family against a finalized native database.
//!
//! These run *beside* [`crate::infrastructure::database::hardware_archive`] and
//! [`crate::infrastructure::database::archive_queries::select_data_archive_series`],
//! not instead of them, for the reason given in
//! [`super::process_stats`]: SQLite stays authoritative, and keeping both
//! callable is what lets a test put one fixture through each path and compare
//! the answers bit for bit.

use chrono::{DateTime, Duration, Utc};
use duckdb::params;
use duckdb::types::Value;

use super::NativeDatabaseError;
use super::binding::sqlite_real_binding;
use super::cell::quote_identifier;
use super::process_stats::sqlite_timestamp_text;
use super::runtime::{NativeCancellation, NativeDatabase};
use super::series::{EPOCH_COLUMN, NativeSeriesQuery, NativeSeriesWindow, select_series};
use super::write_stamp::stamp_for_write;
use crate::infrastructure::database::archive_queries::{
  ArchiveSeriesPoint, DataArchiveColumn, format_datetime,
};
use crate::persistence::archive_data::HardwareArchiveRow;

const TABLE: &str = "DATA_ARCHIVE";

/// The measurement columns whose SQLite declaration is INTEGER but whose
/// writers bind `Option<f32>`, so a finalized archive holds both storage
/// classes in them as `UNION(i BIGINT, r DOUBLE)`. Every other measurement
/// column is single-class DOUBLE in the stable schema.
const TAGGED_COLUMNS: [&str; 6] = [
  "cpu_avg", "cpu_max", "cpu_min", "ram_avg", "ram_max", "ram_min",
];

/// The native equivalent of the SQLite query's `CAST(<column> AS REAL)`.
///
/// The SQLite series queries cast every cell to REAL *before* aggregating, so
/// the mixed storage classes of a tagged column are already reduced to binary64
/// by the time SQLite sums or compares them. Extracting the union's members and
/// converting the integer one the same way reproduces that exactly - including
/// the rounding an integer beyond 2^53 undergoes, which both engines perform as
/// one IEEE conversion. It is deliberately *not* an exact integer/real
/// comparison: reproducing the source means reproducing the cast the source
/// query performs.
pub(super) fn union_member_expression(column: &str) -> String {
  let name = quote_identifier(column);
  format!(
    "COALESCE(CAST(union_extract({name}, 'i') AS DOUBLE), union_extract({name}, 'r'))"
  )
}

/// Every other DATA_ARCHIVE measurement column is single-class DOUBLE, which is
/// already what `CAST(... AS REAL)` would produce.
fn value_expression(column: &str) -> String {
  if TAGGED_COLUMNS.contains(&column) {
    union_member_expression(column)
  } else {
    quote_identifier(column)
  }
}

/// SQLite's INTEGER-affinity rule, which is why these columns hold two storage
/// classes at all.
///
/// A value bound to an INTEGER-affinity column is stored as an INTEGER when it
/// is an exact integer that survives the round trip, and as a REAL otherwise.
/// That is not a legacy quirk the writers outgrew: `Option<f32>` readings go
/// through it on every write today, which is why a finalized archive holds
/// integers in `cpu_avg` beside binary64 readings. `-0.0` converts (SQLite
/// compares the round-tripped value, and `0.0 == -0.0`), so a negative zero
/// reading is stored as the integer 0 - measured, not assumed, in
/// `duckdb_data_archive`.
pub(super) fn sqlite_integer_affinity(value: f64) -> Option<i64> {
  if !(-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&value) {
    return None;
  }
  let candidate = value as i64;
  (candidate as f64 == value).then_some(candidate)
}

/// The placeholder and bound parameter for one reading going into a tagged
/// column, under the rule above.
///
/// NaN is decided first, and not by the affinity rule: SQLite has no NaN, so
/// there is no class for the rule to choose between - the reading is absent
/// (see [`super::binding`]). `±Infinity` is a value in both engines and takes
/// the ordinary path.
pub(super) fn tagged_reading_placeholder(
  reading: f32,
  parameters: &mut Vec<Value>,
) -> String {
  // The same widening sqlx performs when it binds an `f32`, so the value the
  // rules below see is the value SQLite sees.
  let Some(value) = sqlite_real_binding(f64::from(reading)) else {
    return "NULL".to_owned();
  };
  match sqlite_integer_affinity(value) {
    Some(integer) => {
      parameters.push(Value::BigInt(integer));
      "union_value(i := CAST(? AS BIGINT))".to_owned()
    }
    None => {
      parameters.push(Value::Double(value));
      "union_value(r := CAST(? AS DOUBLE))".to_owned()
    }
  }
}

/// Insert one archived minute, stamped with the write cycle's own `timestamp`,
/// the same boundary and the same shared instant as
/// [`crate::infrastructure::database::hardware_archive::insert`].
///
/// The stored bytes are the ones sqlx stores for the same `DateTime<Utc>`, and
/// the derived `__hv_timestamp_epoch_ms` key is what finalization would have
/// derived from those bytes, because it comes from the same oracle (see
/// [`super::write_stamp`]), so a row written here is indistinguishable from a
/// converted one.
///
/// A reading going into one of the INTEGER-declared columns is stored in the
/// class SQLite's affinity rule would have given it (see
/// [`sqlite_integer_affinity`]), and a NaN reading is stored as the gap SQLite
/// stores (see [`super::binding`]), so the row is the row the SQLite writer
/// would have produced rather than one that merely reads back the same.
pub async fn insert(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  row: HardwareArchiveRow,
  timestamp: DateTime<Utc>,
) -> Result<(), NativeDatabaseError> {
  let measurements = [
    ("cpu_avg", row.cpu.avg),
    ("cpu_max", row.cpu.max),
    ("cpu_min", row.cpu.min),
    ("ram_avg", row.memory.avg),
    ("ram_max", row.memory.max),
    ("ram_min", row.memory.min),
    ("cpu_temperature_avg", row.cpu_temperature.avg),
    ("cpu_temperature_max", row.cpu_temperature.max),
    ("cpu_temperature_min", row.cpu_temperature.min),
    ("cpu_power_avg", row.cpu_power.avg),
    ("cpu_power_max", row.cpu_power.max),
    ("cpu_power_min", row.cpu_power.min),
    ("gpu_power_avg", row.gpu_power.avg),
    ("gpu_power_max", row.gpu_power.max),
    ("gpu_power_min", row.gpu_power.min),
    ("ane_power_avg", row.ane_power.avg),
    ("ane_power_max", row.ane_power.max),
    ("ane_power_min", row.ane_power.min),
    ("package_power_avg", row.package_power.avg),
    ("package_power_max", row.package_power.max),
    ("package_power_min", row.package_power.min),
  ];

  let mut columns = vec!["id".to_owned()];
  let mut placeholders = vec!["?".to_owned()];
  let mut parameters = Vec::new();
  for (column, reading) in measurements {
    columns.push(quote_identifier(column));
    placeholders.push(reading_placeholder(column, reading, &mut parameters));
  }
  columns.push("\"timestamp\"".to_owned());
  placeholders.push("?".to_owned());
  columns.push(quote_identifier(EPOCH_COLUMN));
  placeholders.push("?".to_owned());

  let sql = format!(
    "INSERT INTO {TABLE} ({}) VALUES ({})",
    columns.join(", "),
    placeholders.join(", ")
  );
  database
    .request_write(cancellation, move |context| {
      // Once per write cycle, on the lane that is already blocking: the stored
      // text and the key finalization would derive from it come out of the same
      // SQLite oracle, never out of a Rust reading of the instant.
      let (stamp, epoch_milliseconds) = stamp_for_write(&timestamp)?;
      context.with_transaction(|transaction| {
        let id = transaction.next_id(TABLE)?;
        let mut bound = Vec::with_capacity(parameters.len() + 3);
        bound.push(Value::BigInt(id));
        bound.extend(parameters.iter().cloned());
        bound.push(Value::Text(stamp.clone()));
        bound.push(Value::BigInt(epoch_milliseconds));
        transaction
          .connection()
          .execute(&sql, duckdb::params_from_iter(bound))
          .map_err(|error| {
            NativeDatabaseError::duckdb("insert a hardware archive row", error)
          })?;
        Ok(())
      })
    })
    .await
}

/// An absent reading is written as a literal `NULL` rather than as a bound
/// parameter, because a tagged column's non-null values go in through
/// `union_value` and a NULL union is not a union of a NULL member.
///
/// A NaN reading is absent too, for the reason [`super::binding`] measures:
/// SQLite stores a bound NaN as NULL, so an archive that recorded one as a
/// number would answer a reading where the SQLite archive answers a gap.
fn reading_placeholder(
  column: &str,
  reading: Option<f32>,
  parameters: &mut Vec<Value>,
) -> String {
  let Some(reading) = reading else {
    return "NULL".to_owned();
  };
  if TAGGED_COLUMNS.contains(&column) {
    return tagged_reading_placeholder(reading, parameters);
  }
  let Some(value) = sqlite_real_binding(f64::from(reading)) else {
    return "NULL".to_owned();
  };
  // A REAL-affinity column keeps the binary64 bits sqlx would have stored -
  // with one exception. SQLite writes a REAL whose value is an exact integer
  // using an integer serial type and reads it back through that integer, so a
  // negative zero comes back as `+0.0` (measured: `typeof(r), r` over a REAL
  // column holding `-0.0` answers `real|0.0`). It is the only value the round
  // trip changes, and reproducing it is what makes the stored bits equal.
  parameters.push(Value::Double(if value == 0.0 { 0.0 } else { value }));
  "?".to_owned()
}

/// Delete rows older than the Retention Period, returning how many went.
///
/// The membership rule is SQLite's byte-wise text comparison, reproduced for
/// the reason spelled out in [`super::process_stats::delete_old_data`].
pub async fn delete_old_data(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  retention_days: u32,
) -> Result<u64, NativeDatabaseError> {
  let bound =
    sqlite_timestamp_text(&(Utc::now() - Duration::days(i64::from(retention_days))));
  database
    .request_write(cancellation, move |context| {
      context.with_transaction(|transaction| {
        let deleted = transaction
          .connection()
          .execute(
            &format!("DELETE FROM {TABLE} WHERE \"timestamp\" < ?"),
            params![bound.as_str()],
          )
          .map_err(|error| {
            NativeDatabaseError::duckdb("delete expired hardware archive rows", error)
          })?;
        Ok(deleted as u64)
      })
    })
    .await
}

/// The native form of
/// [`crate::infrastructure::database::archive_queries::select_data_archive_series`].
///
/// The range membership stays a byte-wise text comparison because that is what
/// the SQLite query performs: `timestamp` has NUMERIC affinity, the rendered
/// bound is not a number, so the comparison runs under BINARY collation and a
/// caller's spelling selects exactly the stored spellings it selects today.
/// Only the bucket grid moves to the derived epoch key, which is where the
/// SQLite query itself leaves text behind for `strftime`.
pub async fn select_data_archive_series(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  column: DataArchiveColumn,
  window: NativeSeriesWindow<'_>,
) -> Result<Vec<ArchiveSeriesPoint>, NativeDatabaseError> {
  let bounds = window.bounds()?;
  let query = NativeSeriesQuery {
    table: TABLE,
    value_expression: value_expression(column.sql()),
    aggregation: column.aggregation(),
    predicate: "\"timestamp\" BETWEEN ? AND ?".to_owned(),
    parameters: vec![
      Value::Text(format_datetime(window.start)),
      Value::Text(format_datetime(window.end)),
    ],
    bucket_timestamp: window.bucket_timestamp,
    bucket_width_ms: window.bucket_width_ms,
  };
  select_series(database, cancellation, query, bounds).await
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn only_the_mixed_class_columns_extract_union_members() {
    assert_eq!(
      value_expression("cpu_avg"),
      "COALESCE(CAST(union_extract(\"cpu_avg\", 'i') AS DOUBLE), \
       union_extract(\"cpu_avg\", 'r'))"
    );
    assert_eq!(
      value_expression("cpu_temperature_avg"),
      "\"cpu_temperature_avg\""
    );
  }

  #[test]
  fn a_missing_reading_is_a_null_column_not_a_union_of_null() {
    let mut parameters = Vec::new();
    assert_eq!(
      reading_placeholder("cpu_avg", None, &mut parameters),
      "NULL"
    );
    assert!(parameters.is_empty());
    assert_eq!(
      reading_placeholder("cpu_avg", Some(0.5), &mut parameters),
      "union_value(r := CAST(? AS DOUBLE))"
    );
    assert_eq!(parameters, vec![Value::Double(0.5)]);
  }

  /// The rule SQLite applies on the way in, including the negative zero that
  /// is the reason a test noticed the writers needed it at all.
  #[test]
  fn an_integral_reading_takes_the_class_sqlite_affinity_gives_it() {
    let mut parameters = Vec::new();
    assert_eq!(
      reading_placeholder("cpu_avg", Some(-0.0), &mut parameters),
      "union_value(i := CAST(? AS BIGINT))"
    );
    assert_eq!(parameters, vec![Value::BigInt(0)]);

    assert_eq!(sqlite_integer_affinity(2.0), Some(2));
    assert_eq!(sqlite_integer_affinity(-0.0), Some(0));
    assert_eq!(sqlite_integer_affinity(0.5), None);
    assert_eq!(sqlite_integer_affinity(f64::from(f32::MAX)), None);
    assert_eq!(sqlite_integer_affinity(f64::NAN), None);
    assert_eq!(sqlite_integer_affinity(f64::INFINITY), None);

    // A REAL-affinity column is untouched by the rule, except that it cannot
    // hold a negative zero either.
    let mut parameters = Vec::new();
    assert_eq!(
      reading_placeholder("cpu_temperature_avg", Some(2.0), &mut parameters),
      "?"
    );
    assert_eq!(
      reading_placeholder("cpu_temperature_avg", Some(-0.0), &mut parameters),
      "?"
    );
    assert_eq!(parameters, vec![Value::Double(2.0), Value::Double(0.0)]);
    let Value::Double(stored) = parameters[1] else {
      panic!("a REAL-affinity reading must bind as a double");
    };
    assert!(stored.is_sign_positive());
  }

  /// A NaN reading is a gap in both paths, because that is what SQLite stores;
  /// an infinite one is a value in both, because that is what SQLite stores
  /// too. Deciding NaN before the affinity rule is what keeps the second claim
  /// from swallowing the first.
  #[test]
  fn a_nan_reading_is_absent_in_both_paths_and_an_infinite_one_is_not() {
    for column in ["cpu_avg", "cpu_temperature_avg"] {
      let mut parameters = Vec::new();
      assert_eq!(
        reading_placeholder(column, Some(f32::NAN), &mut parameters),
        "NULL",
        "{column}"
      );
      assert_eq!(
        reading_placeholder(column, Some(-f32::NAN), &mut parameters),
        "NULL",
        "{column}"
      );
      // A literal NULL and no bound parameter, exactly as an absent reading.
      assert!(parameters.is_empty(), "{column}");

      assert_ne!(
        reading_placeholder(column, Some(f32::INFINITY), &mut parameters),
        "NULL",
        "{column}"
      );
      assert_ne!(
        reading_placeholder(column, Some(f32::NEG_INFINITY), &mut parameters),
        "NULL",
        "{column}"
      );
      assert_eq!(
        parameters,
        vec![
          Value::Double(f64::INFINITY),
          Value::Double(f64::NEG_INFINITY)
        ],
        "{column}"
      );
    }
  }
}
