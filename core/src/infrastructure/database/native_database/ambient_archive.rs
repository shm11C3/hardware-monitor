//! The `AMBIENT_ARCHIVE` family against a finalized native database.
//!
//! Runs beside [`crate::infrastructure::database::ambient_archive`] and the
//! ambient lane of
//! [`crate::infrastructure::database::archive_queries`], never instead of them:
//! SQLite stays authoritative, and keeping both callable is what lets a test
//! put one fixture through each path and compare the answers bit for bit.

use chrono::{DateTime, Duration, Utc};
use duckdb::params;

use super::NativeDatabaseError;
use super::archive_sql::{AMBIENT_EPOCH_MS, DATA_EPOCH_MS, bucket_of_epoch, minute_key};
use super::binding::{required_real, sqlite_real_binding};
use super::process_stats::sqlite_timestamp_text;
use super::runtime::{NativeCancellation, NativeDatabase};
use super::write_stamp::write_stamp;
use crate::infrastructure::database::archive_queries::{
  AggregatedAmbientBucket, AmbientArchiveSeries, ArchiveBucketTimestamp,
  ArchiveSeriesBounds, fill_ambient_archive_series,
};
use crate::persistence::archive_data::AmbientData;

const TABLE: &str = "AMBIENT_ARCHIVE";

/// One archive minute's ambient rows, all stamped with the cycle's own
/// `timestamp`, in a single transaction - the same boundary and the same shared
/// instant as the SQLite writer.
///
/// A minute with no usable reading writes no row: nothing here fills a gap, and
/// a NULL humidity stays NULL rather than becoming a measured 0%.
pub async fn insert(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  rows: Vec<AmbientData>,
  timestamp: DateTime<Utc>,
) -> Result<(), NativeDatabaseError> {
  if rows.is_empty() {
    return Ok(());
  }
  let (timestamp_text, timestamp_epoch_ms) = write_stamp(timestamp).await?;
  database
    .request_write(cancellation, move |context| {
      context.with_transaction(|transaction| {
        let connection = transaction.connection();
        let mut statement = connection
          .prepare(
            "INSERT INTO AMBIENT_ARCHIVE \
             (id, source, temperature, humidity, timestamp, __hv_timestamp_epoch_ms) \
             VALUES (?, ?, ?, ?, ?, ?)",
          )
          .map_err(|error| {
            NativeDatabaseError::duckdb("prepare the ambient archive insert", error)
          })?;
        for row in &rows {
          transaction.check_cancelled()?;
          let id = transaction.next_id(TABLE)?;
          statement
            .execute(params![
              id,
              row.source.as_str(),
              required_real(TABLE, "temperature", f64::from(row.temperature))?,
              row
                .humidity
                .and_then(|humidity| { sqlite_real_binding(f64::from(humidity)) }),
              timestamp_text.as_str(),
              timestamp_epoch_ms
            ])
            .map_err(|error| {
              NativeDatabaseError::duckdb("insert an ambient archive row", error)
            })?;
        }
        Ok(())
      })
    })
    .await
}

/// Delete rows older than the Retention Period, returning how many went.
///
/// The same byte-wise rule as every other raw archive retention: SQLite
/// compares the rendered bound against the stored text under the BINARY
/// collation, and DuckDB's VARCHAR comparison is the same byte order.
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
            "DELETE FROM AMBIENT_ARCHIVE WHERE timestamp < ?",
            params![bound.as_str()],
          )
          .map_err(|error| {
            NativeDatabaseError::duckdb("delete expired ambient archive rows", error)
          })?;
        Ok(deleted as u64)
      })
    })
    .await
}

/// The native form of
/// [`crate::infrastructure::database::archive_queries::select_ambient_archive_series`].
///
/// The pairing rule is structural here exactly as it is in SQLite: each side
/// collapses to one value per minute *before* the `LEFT JOIN`, so a bucket ΔT
/// is the mean of per-minute differences and never the difference of two
/// independently aggregated means. `AVG` skips NULLs, so an ambient minute with
/// no CPU temperature beside it still counts towards `ambient_avg` while
/// dropping out of `delta_avg`.
///
/// The readings and the labels are read in one transaction for the same reason
/// the SQLite query uses one: the lane reads `sources` as evidence that a
/// sensor contributed to this window, and two independent reads could straddle
/// an archive commit and report a label whose rows the bucket query never saw.
pub async fn select_ambient_archive_series(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  start: &DateTime<Utc>,
  end: &DateTime<Utc>,
  bucket_width_ms: i64,
  bucket_timestamp: ArchiveBucketTimestamp,
) -> Result<AmbientArchiveSeries, NativeDatabaseError> {
  let bounds = ArchiveSeriesBounds::new(start, end, bucket_width_ms, bucket_timestamp)
    .map_err(|source| NativeDatabaseError::ArchiveSeries { source })?;
  let (start_ms, end_ms) = (start.timestamp_millis(), end.timestamp_millis());
  let ambient_minute_key = minute_key(AMBIENT_EPOCH_MS);
  let data_minute_key = minute_key(DATA_EPOCH_MS);
  let bucket =
    bucket_of_epoch(bucket_timestamp, "ambient.minute_epoch_ms", bucket_width_ms);
  let buckets_sql = format!(
    "WITH ambient AS (
       SELECT {ambient_minute_key} AS minute_key,
              MIN({AMBIENT_EPOCH_MS}) AS minute_epoch_ms,
              AVG(CAST(AMBIENT_ARCHIVE.temperature AS DOUBLE)) AS ambient_value
       FROM AMBIENT_ARCHIVE
       WHERE {AMBIENT_EPOCH_MS} BETWEEN {start_ms} AND {end_ms}
       GROUP BY minute_key
     ),
     cpu AS (
       SELECT {data_minute_key} AS minute_key,
              AVG(CAST(DATA_ARCHIVE.cpu_temperature_avg AS DOUBLE)) AS cpu_value
       FROM DATA_ARCHIVE
       WHERE {DATA_EPOCH_MS} BETWEEN {start_ms} AND {end_ms}
         AND DATA_ARCHIVE.cpu_temperature_avg IS NOT NULL
       GROUP BY minute_key
     )
     SELECT {bucket} AS timestamp,
            AVG(ambient.ambient_value) AS ambient_avg,
            AVG(cpu.cpu_value - ambient.ambient_value) AS delta_avg,
            COUNT(ambient.ambient_value) AS minute_count
     FROM ambient
     LEFT JOIN cpu ON cpu.minute_key = ambient.minute_key
     GROUP BY 1
     ORDER BY 1 ASC"
  );
  let sources_sql = format!(
    "SELECT DISTINCT source
     FROM AMBIENT_ARCHIVE
     WHERE {AMBIENT_EPOCH_MS} BETWEEN {start_ms} AND {end_ms}
     ORDER BY source ASC"
  );

  database
    .request_read(cancellation, move |context| {
      context.with_transaction(|transaction| {
        let connection = transaction.connection();
        let mut statement = connection.prepare(&buckets_sql).map_err(|error| {
          NativeDatabaseError::duckdb("prepare the ambient series query", error)
        })?;
        let rows = statement
          .query_map([], |row| {
            Ok(AggregatedAmbientBucket {
              timestamp: row.get(0)?,
              ambient_avg: row.get(1)?,
              delta_avg: row.get(2)?,
              minute_count: row.get(3)?,
            })
          })
          .map_err(|error| {
            NativeDatabaseError::duckdb("run the ambient series query", error)
          })?
          .collect::<Result<Vec<_>, _>>()
          .map_err(|error| {
            NativeDatabaseError::duckdb("decode an ambient series bucket", error)
          })?;

        transaction.check_cancelled()?;
        let mut statement = connection.prepare(&sources_sql).map_err(|error| {
          NativeDatabaseError::duckdb("prepare the ambient source query", error)
        })?;
        let sources = statement
          .query_map([], |row| row.get::<_, String>(0))
          .map_err(|error| {
            NativeDatabaseError::duckdb("run the ambient source query", error)
          })?
          .collect::<Result<Vec<_>, _>>()
          .map_err(|error| {
            NativeDatabaseError::duckdb("decode an ambient source label", error)
          })?;

        Ok(AmbientArchiveSeries {
          sources,
          buckets: fill_ambient_archive_series(rows, bounds),
        })
      })
    })
    .await
}
