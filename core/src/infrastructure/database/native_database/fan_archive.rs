//! The `FAN_ARCHIVE` family against a finalized native database.
//!
//! Runs beside [`crate::infrastructure::database::fan_archive`] and the fan
//! lane of [`crate::infrastructure::database::archive_queries`], never instead
//! of them.
//!
//! Row-per-fan, exactly as in SQLite: only a fan that reported an archivable
//! reading gets a row, and a real 0 RPM Inactive Fan Reading is stored as the
//! observation it is rather than treated as a gap.

use chrono::{DateTime, Duration, Utc};
use duckdb::params;

use super::NativeDatabaseError;
use super::archive_sql::{FAN_EPOCH_MS, bucket_of_epoch};
use super::process_stats::sqlite_timestamp_text;
use super::runtime::{NativeCancellation, NativeDatabase};
use super::stored_text;
use super::write_stamp::write_stamp;
use crate::infrastructure::database::archive_queries::{
  AggregatedArchiveBucket, ArchiveBucketTimestamp, ArchiveSeriesBounds, FanArchiveSeries,
  fill_archive_series,
};
use crate::persistence::archive_data::FanArchiveRow;
use crate::persistence::cooling_fan_rollup::FanArchiveMinuteSample;

const TABLE: &str = "FAN_ARCHIVE";

/// Write one archive interval's fan rows, all stamped `timestamp`, in a single
/// transaction: a partially written minute would show some fans dropping out of
/// the lane for a single bucket, which reads as a sensor glitch that never
/// happened.
pub async fn insert(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  rows: Vec<FanArchiveRow>,
  timestamp: DateTime<Utc>,
) -> Result<(), NativeDatabaseError> {
  let (timestamp_text, timestamp_epoch_ms) = write_stamp(timestamp).await?;
  database
    .request_write(cancellation, move |context| {
      context.with_transaction(|transaction| {
        let connection = transaction.connection();
        let mut statement = connection
          .prepare(
            "INSERT INTO FAN_ARCHIVE \
             (id, source, rpm, timestamp, __hv_timestamp_epoch_ms) \
             VALUES (?, ?, ?, ?, ?)",
          )
          .map_err(|error| {
            NativeDatabaseError::duckdb("prepare the fan archive insert", error)
          })?;
        for row in &rows {
          transaction.check_cancelled()?;
          let id = transaction.next_id(TABLE)?;
          statement
            .execute(params![
              id,
              row.source.as_str(),
              i64::from(row.rpm),
              timestamp_text.as_str(),
              timestamp_epoch_ms
            ])
            .map_err(|error| {
              NativeDatabaseError::duckdb("insert a fan archive row", error)
            })?;
        }
        Ok(())
      })
    })
    .await
}

/// Delete rows older than the Retention Period, returning how many went. Same
/// byte-wise bound as [`super::ambient_archive::delete_old_data`].
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
            "DELETE FROM FAN_ARCHIVE WHERE timestamp < ?",
            params![bound.as_str()],
          )
          .map_err(|error| {
            NativeDatabaseError::duckdb("delete expired fan archive rows", error)
          })?;
        Ok(deleted as u64)
      })
    })
    .await
}

/// Every archived fan reading in `[start, end)`, oldest first, for the daily
/// rollup's own pass.
pub async fn select_fan_minutes_for_range(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  start: &DateTime<Utc>,
  end: &DateTime<Utc>,
) -> Result<Vec<FanArchiveMinuteSample>, NativeDatabaseError> {
  let (start_ms, end_ms) = (start.timestamp_millis(), end.timestamp_millis());
  let sql = format!(
    "SELECT timestamp, source, CAST(rpm AS BIGINT) AS rpm
     FROM FAN_ARCHIVE
     WHERE {FAN_EPOCH_MS} >= {start_ms} AND {FAN_EPOCH_MS} < {end_ms}
     ORDER BY timestamp ASC, id ASC"
  );
  database
    .request_read(cancellation, move |context| {
      context.check_cancelled()?;
      let mut statement = context.connection().prepare(&sql).map_err(|error| {
        NativeDatabaseError::duckdb("prepare the fan minute query", error)
      })?;
      let rows = statement
        .query_map([], |row| {
          Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
          ))
        })
        .map_err(|error| NativeDatabaseError::duckdb("run the fan minute query", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| NativeDatabaseError::duckdb("decode a fan minute row", error))?;
      rows
        .into_iter()
        .map(|(timestamp, source, rpm)| {
          Ok(FanArchiveMinuteSample {
            timestamp: stored_text::datetime(TABLE, "timestamp", &timestamp)?,
            source,
            // The column is only ever written from a `u32`; clamp rather than
            // wrap if a hand-edited database ever carries a negative.
            rpm: rpm.max(0) as u32,
          })
        })
        .collect()
    })
    .await
}

/// Whether the one-minute fan archive currently holds any reading at all - the
/// evidence that separates "this machine has no readable fan" from "the daily
/// rollup has not summarized the fan yet". Deliberately unbounded in time, as
/// in SQLite.
pub async fn has_any_reading(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
) -> Result<bool, NativeDatabaseError> {
  database
    .request_read(cancellation, move |context| {
      context.check_cancelled()?;
      context
        .connection()
        .query_row("SELECT EXISTS(SELECT 1 FROM FAN_ARCHIVE)", [], |row| {
          row.get::<_, bool>(0)
        })
        .map_err(|error| {
          NativeDatabaseError::duckdb("look for any archived fan reading", error)
        })
    })
    .await
}

/// The most recent archived fan timestamp strictly before `before`.
///
/// Membership is decided by the derived epoch key, but the answer is
/// `MAX(timestamp)` over the stored text - a byte-wise maximum in both engines,
/// so a database mixing timestamp spellings picks the same row either way.
pub async fn max_fan_archive_timestamp_before(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  before: &DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, NativeDatabaseError> {
  let before_ms = before.timestamp_millis();
  let sql =
    format!("SELECT MAX(timestamp) FROM FAN_ARCHIVE WHERE {FAN_EPOCH_MS} < {before_ms}");
  database
    .request_read(cancellation, move |context| {
      context.check_cancelled()?;
      let latest: Option<String> = context
        .connection()
        .query_row(&sql, [], |row| row.get(0))
        .map_err(|error| {
          NativeDatabaseError::duckdb("read the latest archived fan timestamp", error)
        })?;
      latest
        .map(|text| stored_text::datetime(TABLE, "timestamp", &text))
        .transpose()
    })
    .await
}

/// The native form of
/// [`crate::infrastructure::database::archive_queries::select_fan_archive_series`].
///
/// One round trip for every fan, grouped in Rust: the rows already arrive
/// ordered by source, and each source's buckets then go through the same
/// gap-filling every other archive series uses.
pub async fn select_fan_archive_series(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  start: &DateTime<Utc>,
  end: &DateTime<Utc>,
  bucket_width_ms: i64,
  bucket_timestamp: ArchiveBucketTimestamp,
) -> Result<Vec<FanArchiveSeries>, NativeDatabaseError> {
  let bounds = ArchiveSeriesBounds::new(start, end, bucket_width_ms, bucket_timestamp)
    .map_err(|source| NativeDatabaseError::ArchiveSeries { source })?;
  let (start_ms, end_ms) = (start.timestamp_millis(), end.timestamp_millis());
  let bucket = bucket_of_epoch(bucket_timestamp, FAN_EPOCH_MS, bucket_width_ms);
  let sql = format!(
    "SELECT source,
            {bucket} AS timestamp,
            AVG(CAST(rpm AS DOUBLE)) AS value,
            COUNT(rpm) AS value_count
     FROM FAN_ARCHIVE
     WHERE {FAN_EPOCH_MS} BETWEEN {start_ms} AND {end_ms}
     GROUP BY source, 2
     ORDER BY source ASC, 2 ASC"
  );
  database
    .request_read(cancellation, move |context| {
      context.check_cancelled()?;
      let mut statement = context.connection().prepare(&sql).map_err(|error| {
        NativeDatabaseError::duckdb("prepare the fan series query", error)
      })?;
      let rows = statement
        .query_map([], |row| {
          Ok((
            row.get::<_, String>(0)?,
            AggregatedArchiveBucket {
              timestamp: row.get(1)?,
              value: row.get(2)?,
              value_count: row.get(3)?,
            },
          ))
        })
        .map_err(|error| NativeDatabaseError::duckdb("run the fan series query", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
          NativeDatabaseError::duckdb("decode a fan series bucket", error)
        })?;

      let mut series: Vec<FanArchiveSeries> = Vec::new();
      let mut current: Vec<AggregatedArchiveBucket> = Vec::new();
      let mut current_source: Option<String> = None;
      for (source, bucket) in rows {
        if current_source.as_deref() != Some(source.as_str()) {
          if let Some(previous) = current_source.take() {
            series.push(FanArchiveSeries {
              source: previous,
              points: fill_archive_series(std::mem::take(&mut current), bounds),
            });
          }
          current_source = Some(source);
        }
        current.push(bucket);
      }
      if let Some(source) = current_source {
        series.push(FanArchiveSeries {
          source,
          points: fill_archive_series(current, bounds),
        });
      }
      Ok(series)
    })
    .await
}
