//! The GPU_DATA_ARCHIVE family against a finalized native database.
//!
//! Runs beside [`crate::infrastructure::database::gpu_archive`] and the GPU
//! queries in [`crate::infrastructure::database::archive_queries`], for the
//! reason given in [`super::process_stats`].
//!
//! GPU series stay **name-based** and `gpu_id` stays an opaque stored value:
//! ADR 0019 keeps the archived name as the subject a user picked, and two
//! adapters that once reported the same name are already one archived subject.
//! Nothing here reconstructs a live inventory id or splits history by one.

use chrono::{DateTime, Duration, Utc};
use duckdb::params;
use duckdb::types::Value;

use super::NativeDatabaseError;
use super::cell::quote_identifier;
use super::data_archive::{tagged_reading_placeholder, union_member_expression};
use super::process_stats::sqlite_timestamp_text;
use super::runtime::{NativeCancellation, NativeDatabase};
use super::series::{EPOCH_COLUMN, NativeSeriesQuery, NativeSeriesWindow, select_series};
use super::write_stamp::stamp_for_write;
use crate::infrastructure::database::archive_queries::{
  ArchiveSeriesPoint, GpuArchiveColumn, format_datetime,
};
use crate::persistence::archive_data::GpuData;

const TABLE: &str = "GPU_DATA_ARCHIVE";

/// The GPU measurement columns the writers bind as `Option<f32>`, which is why
/// the stable schema holds them as `UNION(i BIGINT, r DOUBLE)`. The remaining
/// ones are written from `Option<i32>` and stay single-class BIGINT, so the
/// SQLite query's `CAST(... AS REAL)` becomes an explicit conversion instead of
/// a member extraction.
const TAGGED_COLUMNS: [&str; 4] =
  ["usage_avg", "usage_max", "usage_min", "temperature_avg"];

fn value_expression(column: &str) -> String {
  if TAGGED_COLUMNS.contains(&column) {
    union_member_expression(column)
  } else {
    format!("CAST({} AS DOUBLE)", quote_identifier(column))
  }
}

/// Insert one archived GPU minute - see [`super::data_archive::insert`] for the
/// shared instant, the stored timestamp bytes and the derived epoch key.
pub async fn insert(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  data: GpuData,
  timestamp: DateTime<Utc>,
) -> Result<(), NativeDatabaseError> {
  let readings = [
    ("usage_avg", data.usage_avg),
    ("usage_max", data.usage_max),
    ("usage_min", data.usage_min),
    ("temperature_avg", data.temperature_avg),
  ];
  let counts = [
    ("temperature_max", data.temperature_max),
    ("temperature_min", data.temperature_min),
    ("dedicated_memory_avg", data.dedicated_memory_avg),
    ("dedicated_memory_max", data.dedicated_memory_max),
    ("dedicated_memory_min", data.dedicated_memory_min),
  ];

  let mut columns = vec!["id".to_owned(), "gpu_id".to_owned(), "gpu_name".to_owned()];
  let mut placeholders = vec!["?".to_owned(), "?".to_owned(), "?".to_owned()];
  let mut parameters = vec![
    data.gpu_id.map_or(Value::Null, Value::Text),
    Value::Text(data.gpu_name),
  ];
  for (column, reading) in readings {
    columns.push(quote_identifier(column));
    match reading {
      // A tagged column takes the class SQLite's INTEGER affinity would have
      // given the reading; an absent one stays a NULL column rather than
      // becoming a union of a NULL member.
      Some(reading) => {
        placeholders.push(tagged_reading_placeholder(reading, &mut parameters));
      }
      None => placeholders.push("NULL".to_owned()),
    }
  }
  for (column, count) in counts {
    columns.push(quote_identifier(column));
    placeholders.push("?".to_owned());
    parameters.push(count.map_or(Value::Null, |count| Value::BigInt(i64::from(count))));
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
      // See `super::data_archive::insert`: the stamp and its derived key come
      // from SQLite, once per write cycle, on this blocking lane.
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
            NativeDatabaseError::duckdb("insert a GPU archive row", error)
          })?;
        Ok(())
      })
    })
    .await
}

/// Delete rows older than the Retention Period, returning how many went. The
/// membership rule is the byte-wise text comparison SQLite performs; see
/// [`super::process_stats::delete_old_data`].
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
            NativeDatabaseError::duckdb("delete expired GPU archive rows", error)
          })?;
        Ok(deleted as u64)
      })
    })
    .await
}

/// The native form of
/// [`crate::infrastructure::database::archive_queries::select_gpu_archive_series`].
///
/// `gpu_name` is matched by exact stored bytes, including an embedded NUL,
/// because that is the subject key the SQLite query uses.
pub async fn select_gpu_archive_series(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  column: GpuArchiveColumn,
  gpu_name: &str,
  window: NativeSeriesWindow<'_>,
) -> Result<Vec<ArchiveSeriesPoint>, NativeDatabaseError> {
  let bounds = window.bounds()?;
  let query = NativeSeriesQuery {
    table: TABLE,
    value_expression: value_expression(column.sql()),
    aggregation: column.aggregation(),
    predicate: "gpu_name = ? AND \"timestamp\" BETWEEN ? AND ?".to_owned(),
    parameters: vec![
      Value::Text(gpu_name.to_owned()),
      Value::Text(format_datetime(window.start)),
      Value::Text(format_datetime(window.end)),
    ],
    bucket_timestamp: window.bucket_timestamp,
    bucket_width_ms: window.bucket_width_ms,
  };
  select_series(database, cancellation, query, bounds).await
}

/// The native form of
/// [`crate::infrastructure::database::archive_queries::select_gpu_names`].
///
/// `'Unknown'` is excluded the way the SQLite query excludes it: as a stored
/// value that names no adapter, not as a missing row. DuckDB orders VARCHAR by
/// the same byte order SQLite's BINARY collation uses, so the returned order is
/// the source's.
pub async fn select_gpu_names(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
) -> Result<Vec<String>, NativeDatabaseError> {
  database
    .request_read(cancellation, move |context| {
      context.check_cancelled()?;
      let mut statement = context
        .connection()
        .prepare(
          "SELECT DISTINCT gpu_name
           FROM GPU_DATA_ARCHIVE
           WHERE gpu_name IS NOT NULL
             AND gpu_name != 'Unknown'
           ORDER BY gpu_name ASC",
        )
        .map_err(|error| {
          NativeDatabaseError::duckdb("prepare the GPU name query", error)
        })?;
      statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| NativeDatabaseError::duckdb("run the GPU name query", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| NativeDatabaseError::duckdb("decode a GPU name", error))
    })
    .await
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn single_class_gpu_columns_convert_instead_of_extracting_a_member() {
    assert_eq!(
      value_expression("usage_avg"),
      "COALESCE(CAST(union_extract(\"usage_avg\", 'i') AS DOUBLE), \
       union_extract(\"usage_avg\", 'r'))"
    );
    assert_eq!(
      value_expression("dedicated_memory_max"),
      "CAST(\"dedicated_memory_max\" AS DOUBLE)"
    );
  }
}
