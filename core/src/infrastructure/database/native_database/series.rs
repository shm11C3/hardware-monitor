//! The bucketed archive series query against a finalized native database.
//!
//! Shared by the DATA_ARCHIVE and GPU_DATA_ARCHIVE families because the two
//! SQLite queries they reproduce differ only in the table, the filtered column
//! and how one measurement column is spelled - everything that decides what a
//! point *means* (the bucket grid, the endpoints, the gap convention and the
//! point limit) is the same rule, and a second copy of it would be a second
//! place for it to drift.
//!
//! The bucket grid is not recomputed here either: the bounds, the gap filling
//! and the `MAX_ARCHIVE_SERIES_POINTS` refusal come from
//! [`crate::infrastructure::database::archive_queries`], so the native series
//! can only differ from the SQLite one in the aggregate it reads out of the
//! engine.

use chrono::{DateTime, Utc};
use duckdb::types::Value;

use super::NativeDatabaseError;
use super::cell::quote_identifier;
use super::runtime::{NativeCancellation, NativeDatabase};
use crate::infrastructure::database::archive_queries::{
  AggregatedArchiveBucket, ArchiveAggregation, ArchiveBucketTimestamp,
  ArchiveSeriesBounds, ArchiveSeriesPoint, fill_archive_series,
};

/// The range a caller asked for and the grid it asked for it on - the four
/// arguments every archive series query takes, whichever family it belongs to.
///
/// They travel together because they are only meaningful together: the bucket
/// grid is what turns a range into points, and validating one without the
/// others is what produced the refusals the families share.
#[derive(Debug, Clone, Copy)]
pub struct NativeSeriesWindow<'a> {
  pub start: &'a DateTime<Utc>,
  pub end: &'a DateTime<Utc>,
  pub bucket_width_ms: i64,
  pub bucket_timestamp: ArchiveBucketTimestamp,
}

impl NativeSeriesWindow<'_> {
  /// The grid, built by `archive_queries` so an invalid range, a non-positive
  /// bucket width and an over-long series are refused before any engine work,
  /// for exactly the SQLite path's reason - the native families return their
  /// own error type, so that reason travels wrapped rather than restated.
  pub(super) fn bounds(&self) -> Result<ArchiveSeriesBounds, NativeDatabaseError> {
    ArchiveSeriesBounds::new(
      self.start,
      self.end,
      self.bucket_width_ms,
      self.bucket_timestamp,
    )
    .map_err(|source| NativeDatabaseError::ArchiveSeries { source })
  }
}

/// The derived query key finalization fills from the stored timestamp text.
pub(super) const EPOCH_COLUMN: &str = "__hv_timestamp_epoch_ms";

pub(super) struct NativeSeriesQuery {
  pub(super) table: &'static str,
  /// The native equivalent of the SQLite query's `CAST(<column> AS REAL)`.
  pub(super) value_expression: String,
  pub(super) aggregation: ArchiveAggregation,
  /// The `WHERE` body, consuming `parameters` in order.
  pub(super) predicate: String,
  pub(super) parameters: Vec<Value>,
  pub(super) bucket_timestamp: ArchiveBucketTimestamp,
  pub(super) bucket_width_ms: i64,
}

/// Run one bucketed series query and fill its gaps.
///
/// `bounds` is built by the caller before any engine work, so an invalid range,
/// a non-positive bucket width and an over-long series are refused without
/// opening a request, carrying the SQLite path's own `ArchiveSeriesError`
/// inside `NativeDatabaseError::ArchiveSeries`.
pub(super) async fn select_series(
  database: &NativeDatabase,
  cancellation: NativeCancellation,
  query: NativeSeriesQuery,
  bounds: ArchiveSeriesBounds,
) -> Result<Vec<ArchiveSeriesPoint>, NativeDatabaseError> {
  let rows = database
    .request_read(cancellation, move |context| {
      context.check_cancelled()?;
      query.run(context.connection())
    })
    .await?;
  Ok(fill_archive_series(rows, bounds))
}

impl NativeSeriesQuery {
  fn run(
    &self,
    connection: &duckdb::Connection,
  ) -> Result<Vec<AggregatedArchiveBucket>, NativeDatabaseError> {
    let sql = format!(
      "SELECT {bucket} AS bucket,
              {aggregation}({value}) AS value,
              COUNT({value}) AS value_count
       FROM {table}
       WHERE {predicate}
       GROUP BY 1
       ORDER BY 1 ASC NULLS FIRST",
      bucket = self.bucket_expression(),
      aggregation = self.aggregation.sql(),
      value = self.value_expression,
      table = quote_identifier(self.table),
      predicate = self.predicate,
    );
    let mut statement = connection.prepare(&sql).map_err(|error| {
      NativeDatabaseError::duckdb("prepare the archive series query", error)
    })?;
    let rows = statement
      .query_map(duckdb::params_from_iter(self.parameters.iter()), |row| {
        Ok((
          row.get::<_, Option<i64>>(0)?,
          row.get::<_, Option<f64>>(1)?,
          row.get::<_, i64>(2)?,
        ))
      })
      .map_err(|error| {
        NativeDatabaseError::duckdb("run the archive series query", error)
      })?
      .collect::<Result<Vec<_>, _>>()
      .map_err(|error| {
        NativeDatabaseError::duckdb("decode an archive series bucket", error)
      })?;

    let mut buckets = Vec::with_capacity(rows.len());
    for (bucket, value, value_count) in rows {
      buckets.push(AggregatedArchiveBucket {
        // A row whose stored text SQLite cannot read as an instant has no
        // derived key, so it has no bucket - and that is what the SQLite query
        // answers too, by a route worth naming: it groups the row under a NULL
        // bucket and sqlx decodes that NULL as `0`. Reading it as `0` here too
        // keeps the two engines identical by construction instead of by an
        // argument about which ranges can reach 1970.
        //
        // That is also why the query above orders `NULLS FIRST`: a range wide
        // enough to hold both an epoch-zero row and an unreadable one produces
        // *two* groups that both arrive as timestamp 0, and `fill_archive_series`
        // keeps the first. SQLite sorts NULL first and DuckDB sorts it last, so
        // without the clause the two engines would keep different groups and
        // answer different aggregates for that bucket.
        //
        // How often an unreadable stamp occurs is reported once, at conversion
        // time, as `NativeTableReport::unconvertible_timestamps`.
        timestamp: bucket.unwrap_or(0),
        value,
        value_count,
      });
    }
    Ok(buckets)
  }

  /// The bucket grid, computed from the derived epoch key exactly as
  /// `archive_queries::bucket_of_epoch_sql` computes it from the adapter
  /// expression.
  ///
  /// SQLite's `/` between integers truncates toward zero, while DuckDB's `//`
  /// is only specified to be integer division; pre-epoch rows are where the
  /// two conventions would part. The operands are made non-negative first, so
  /// the grid is the source's regardless of which convention `//` follows.
  fn bucket_expression(&self) -> String {
    let epoch = quote_identifier(EPOCH_COLUMN);
    let width = self.bucket_width_ms;
    let numerator = match self.bucket_timestamp {
      ArchiveBucketTimestamp::Start => epoch,
      ArchiveBucketTimestamp::End => format!("({epoch} + {width} - 1)"),
    };
    format!(
      "(CASE WHEN {numerator} < 0 \
       THEN -((-({numerator})) // {width}) \
       ELSE {numerator} // {width} END) * {width}"
    )
  }
}
