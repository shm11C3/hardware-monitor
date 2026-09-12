//! Stable native DuckDB schema supplied by the App database boundary.
//!
//! This is the authoritative runtime shape, not a translation of SQLite
//! syntax. Candidate finalization
//! ([`hardviz_core::infrastructure::database::native_database::finalize_candidate_database`])
//! copies the validated snapshot into these constrained tables before the
//! result can be exercised as a native backend. App owns the ordered schema
//! definition for the same reason it owns the SQLite migration set; Core owns
//! how the copy, the identity allocator and the queries behave.
//!
//! Original timestamp text stays stored verbatim. The `__hv_timestamp_epoch_ms`
//! columns are derived query keys the finalizer fills from the stored text.

// No App caller reads this definition yet: the finalizer that consumes it is
// Core's, exercised from Core's tests, and App lifecycle only chooses a backend
// in the dependent cutover change. Moving the definition into Core would settle
// the lint and put schema ownership on the wrong side of the boundary that also
// keeps the ordered SQLite migration set here, so the lint is suppressed
// instead.
#![allow(dead_code)]

use hardviz_core::infrastructure::database::native_database::{
  NativeIdentity, NativeIdentityMode, NativeSchemaDefinition, NativeTimestampColumn,
};

pub const NATIVE_SCHEMA_VERSION: u32 = 1;

/// Every current domain table, ordered so a table is created and filled after
/// the tables its foreign keys reference (`storage_health_daily_records`
/// references `storage_devices`).
const TABLES: &[&str] = &[
  "DATA_ARCHIVE",
  "GPU_DATA_ARCHIVE",
  "PROCESS_STATS",
  "storage_devices",
  "storage_health_daily_records",
  "cooling_daily_summary",
  "cooling_baseline",
  "cooling_hourly_summary",
  "AMBIENT_ARCHIVE",
  "FAN_ARCHIVE",
  "cooling_fan_daily_summary",
  "cooling_delta_baseline",
  "cooling_thermal_delta_daily_summary",
  "cooling_covariate_daily_summary",
  "cooling_fan_covariate_daily_summary",
];

/// The four archive tables whose rows are read back by time range. Cooling
/// summaries and Storage Health are keyed by a `date`/`hour_start` string
/// rather than an instant, so they need no derived epoch key.
///
/// `GPU_DATA_ARCHIVE` belongs here for the same reason `DATA_ARCHIVE` does:
/// `archive_queries::gpu_archive_series_sql` buckets its rows through
/// `sqlite_epoch_milliseconds()`, so the native series query needs the same
/// derived key rather than a second reading of SQLite's date-string grammar.
const TIMESTAMPS: &[NativeTimestampColumn] = &[
  NativeTimestampColumn {
    table: "DATA_ARCHIVE",
    source_column: "timestamp",
    epoch_milliseconds_column: "__hv_timestamp_epoch_ms",
  },
  NativeTimestampColumn {
    table: "GPU_DATA_ARCHIVE",
    source_column: "timestamp",
    epoch_milliseconds_column: "__hv_timestamp_epoch_ms",
  },
  NativeTimestampColumn {
    table: "AMBIENT_ARCHIVE",
    source_column: "timestamp",
    epoch_milliseconds_column: "__hv_timestamp_epoch_ms",
  },
  NativeTimestampColumn {
    table: "FAN_ARCHIVE",
    source_column: "timestamp",
    epoch_milliseconds_column: "__hv_timestamp_epoch_ms",
  },
];

/// Which SQLite identity rule each `id` column carries, so the native
/// allocator can reproduce it. `AutoIncrement` names the `sqlite_sequence` row
/// whose high-water mark the finalizer imports; the remaining tables use plain
/// `INTEGER PRIMARY KEY` rowid allocation.
const IDENTITIES: &[NativeIdentity] = &[
  NativeIdentity {
    table: "DATA_ARCHIVE",
    column: "id",
    mode: NativeIdentityMode::RowId,
  },
  NativeIdentity {
    table: "GPU_DATA_ARCHIVE",
    column: "id",
    mode: NativeIdentityMode::RowId,
  },
  NativeIdentity {
    table: "PROCESS_STATS",
    column: "id",
    mode: NativeIdentityMode::AutoIncrement {
      sqlite_sequence_name: "PROCESS_STATS",
    },
  },
  NativeIdentity {
    table: "AMBIENT_ARCHIVE",
    column: "id",
    mode: NativeIdentityMode::AutoIncrement {
      sqlite_sequence_name: "AMBIENT_ARCHIVE",
    },
  },
  NativeIdentity {
    table: "FAN_ARCHIVE",
    column: "id",
    mode: NativeIdentityMode::RowId,
  },
  NativeIdentity {
    table: "storage_health_daily_records",
    column: "id",
    mode: NativeIdentityMode::AutoIncrement {
      sqlite_sequence_name: "storage_health_daily_records",
    },
  },
];

pub fn get_native_schema() -> NativeSchemaDefinition {
  NativeSchemaDefinition {
    version: NATIVE_SCHEMA_VERSION,
    sql: NATIVE_SCHEMA_SQL,
    tables: TABLES,
    timestamp_columns: TIMESTAMPS,
    identities: IDENTITIES,
  }
}

/// The fifteen current application data tables. Finalizer-owned metadata and
/// allocator state live outside these domain tables.
///
/// `UNION(i BIGINT, r DOUBLE)` marks the ten measurement columns whose SQLite
/// declaration is INTEGER but whose production writers bind `Option<f32>`, so
/// existing databases hold both storage classes in the same column (see the
/// Design Doc, "Preserving meaning through conversion"). Converting them to
/// DOUBLE would round large integers; converting them to BIGINT would drop the
/// fractional readings. Every other column is single-class in the source, and
/// finalization refuses - naming the table, column and row - any cell the
/// declared type cannot hold rather than coercing it.
///
/// `__hv_timestamp_epoch_ms` is nullable even where its source `timestamp` is
/// NOT NULL: it must be NULL exactly when SQLite's `strftime` returns NULL for
/// the stored text, and an absent conversion is not a guessable zero (DP-02).
pub const NATIVE_SCHEMA_SQL: &str = r#"
CREATE TABLE DATA_ARCHIVE (
  id BIGINT PRIMARY KEY,
  cpu_avg UNION(i BIGINT, r DOUBLE),
  cpu_max UNION(i BIGINT, r DOUBLE),
  cpu_min UNION(i BIGINT, r DOUBLE),
  ram_avg UNION(i BIGINT, r DOUBLE),
  ram_max UNION(i BIGINT, r DOUBLE),
  ram_min UNION(i BIGINT, r DOUBLE),
  timestamp VARCHAR,
  cpu_temperature_avg DOUBLE,
  cpu_temperature_max DOUBLE,
  cpu_temperature_min DOUBLE,
  cpu_power_avg DOUBLE,
  cpu_power_max DOUBLE,
  cpu_power_min DOUBLE,
  gpu_power_avg DOUBLE,
  gpu_power_max DOUBLE,
  gpu_power_min DOUBLE,
  ane_power_avg DOUBLE,
  ane_power_max DOUBLE,
  ane_power_min DOUBLE,
  package_power_avg DOUBLE,
  package_power_max DOUBLE,
  package_power_min DOUBLE,
  __hv_timestamp_epoch_ms BIGINT
);

CREATE TABLE GPU_DATA_ARCHIVE (
  id BIGINT PRIMARY KEY,
  gpu_name VARCHAR,
  usage_avg UNION(i BIGINT, r DOUBLE),
  usage_max UNION(i BIGINT, r DOUBLE),
  usage_min UNION(i BIGINT, r DOUBLE),
  temperature_avg UNION(i BIGINT, r DOUBLE),
  temperature_max BIGINT,
  temperature_min BIGINT,
  timestamp VARCHAR,
  dedicated_memory_avg BIGINT,
  dedicated_memory_max BIGINT,
  dedicated_memory_min BIGINT,
  gpu_id VARCHAR,
  __hv_timestamp_epoch_ms BIGINT
);

CREATE TABLE PROCESS_STATS (
  id BIGINT PRIMARY KEY,
  pid BIGINT NOT NULL,
  process_name VARCHAR NOT NULL,
  cpu_usage DOUBLE NOT NULL,
  memory_usage BIGINT NOT NULL,
  execution_sec BIGINT NOT NULL,
  timestamp VARCHAR NOT NULL
);

CREATE TABLE storage_devices (
  id VARCHAR PRIMARY KEY,
  display_name VARCHAR NOT NULL,
  model VARCHAR,
  serial_hash VARCHAR,
  protocol VARCHAR,
  capacity_bytes BIGINT,
  first_seen_at VARCHAR NOT NULL,
  last_seen_at VARCHAR NOT NULL,
  is_active BIGINT NOT NULL DEFAULT 1
);

CREATE TABLE storage_health_daily_records (
  id BIGINT PRIMARY KEY,
  device_id VARCHAR NOT NULL,
  date VARCHAR NOT NULL,
  health_status VARCHAR NOT NULL,
  warning_level VARCHAR NOT NULL DEFAULT 'none',
  warning_reasons VARCHAR,
  temperature_celsius DOUBLE,
  power_on_hours BIGINT,
  percentage_used DOUBLE,
  available_spare_percent DOUBLE,
  reallocated_sector_count BIGINT,
  current_pending_sector_count BIGINT,
  offline_uncorrectable_count BIGINT,
  media_errors BIGINT,
  error_log_entries BIGINT,
  unsafe_shutdown_count BIGINT,
  collected_at VARCHAR NOT NULL,
  UNIQUE(device_id, date),
  FOREIGN KEY(device_id) REFERENCES storage_devices(id)
);

CREATE TABLE cooling_daily_summary (
  date VARCHAR PRIMARY KEY,
  idle_cpu_temperature_avg DOUBLE,
  idle_cpu_temperature_max DOUBLE,
  idle_cpu_temperature_min DOUBLE,
  idle_sample_minutes BIGINT NOT NULL DEFAULT 0,
  low_cpu_temperature_avg DOUBLE,
  low_cpu_temperature_max DOUBLE,
  low_cpu_temperature_min DOUBLE,
  low_sample_minutes BIGINT NOT NULL DEFAULT 0,
  mid_cpu_temperature_avg DOUBLE,
  mid_cpu_temperature_max DOUBLE,
  mid_cpu_temperature_min DOUBLE,
  mid_sample_minutes BIGINT NOT NULL DEFAULT 0,
  high_cpu_temperature_avg DOUBLE,
  high_cpu_temperature_max DOUBLE,
  high_cpu_temperature_min DOUBLE,
  high_sample_minutes BIGINT NOT NULL DEFAULT 0,
  coverage_minutes BIGINT NOT NULL,
  cpu_power_avg DOUBLE,
  cpu_power_max DOUBLE,
  cpu_power_min DOUBLE,
  power_sample_minutes BIGINT NOT NULL DEFAULT 0
);

CREATE TABLE cooling_baseline (
  id BIGINT PRIMARY KEY CHECK (id = 1),
  window_start_date VARCHAR NOT NULL,
  window_end_date VARCHAR NOT NULL,
  idle_temperature_avg DOUBLE NOT NULL,
  sample_minutes BIGINT NOT NULL,
  established_at VARCHAR NOT NULL
);

CREATE TABLE cooling_hourly_summary (
  hour_start VARCHAR PRIMARY KEY,
  cpu_usage_avg DOUBLE,
  cpu_temperature_avg DOUBLE,
  sample_minutes BIGINT NOT NULL
);

CREATE TABLE AMBIENT_ARCHIVE (
  id BIGINT PRIMARY KEY,
  source VARCHAR NOT NULL,
  temperature DOUBLE NOT NULL,
  humidity DOUBLE,
  timestamp VARCHAR NOT NULL,
  __hv_timestamp_epoch_ms BIGINT
);

CREATE TABLE FAN_ARCHIVE (
  id BIGINT PRIMARY KEY,
  source VARCHAR NOT NULL,
  rpm BIGINT NOT NULL,
  timestamp VARCHAR NOT NULL,
  __hv_timestamp_epoch_ms BIGINT
);

CREATE TABLE cooling_fan_daily_summary (
  date VARCHAR NOT NULL,
  source VARCHAR NOT NULL,
  rpm_avg DOUBLE NOT NULL,
  rpm_max BIGINT NOT NULL,
  rpm_min BIGINT NOT NULL,
  sample_minutes BIGINT NOT NULL,
  PRIMARY KEY (date, source)
);

CREATE TABLE cooling_delta_baseline (
  id BIGINT PRIMARY KEY CHECK (id = 1),
  source VARCHAR NOT NULL,
  window_start_date VARCHAR NOT NULL,
  window_end_date VARCHAR NOT NULL,
  delta_temperature_avg DOUBLE NOT NULL,
  sample_minutes BIGINT NOT NULL,
  established_at VARCHAR NOT NULL
);

CREATE TABLE cooling_thermal_delta_daily_summary (
  date VARCHAR NOT NULL,
  source VARCHAR NOT NULL,
  coverage_minutes BIGINT NOT NULL,
  idle_delta_temperature_avg DOUBLE,
  idle_delta_temperature_max DOUBLE,
  idle_delta_temperature_min DOUBLE,
  idle_delta_sample_minutes BIGINT NOT NULL DEFAULT 0,
  low_delta_temperature_avg DOUBLE,
  low_delta_temperature_max DOUBLE,
  low_delta_temperature_min DOUBLE,
  low_delta_sample_minutes BIGINT NOT NULL DEFAULT 0,
  mid_delta_temperature_avg DOUBLE,
  mid_delta_temperature_max DOUBLE,
  mid_delta_temperature_min DOUBLE,
  mid_delta_sample_minutes BIGINT NOT NULL DEFAULT 0,
  high_delta_temperature_avg DOUBLE,
  high_delta_temperature_max DOUBLE,
  high_delta_temperature_min DOUBLE,
  high_delta_sample_minutes BIGINT NOT NULL DEFAULT 0,
  PRIMARY KEY (date, source)
);

CREATE TABLE cooling_covariate_daily_summary (
  date VARCHAR NOT NULL,
  source VARCHAR NOT NULL,
  band VARCHAR NOT NULL,
  sample_minutes BIGINT NOT NULL,
  band_share DOUBLE NOT NULL,
  ambient_temperature_median DOUBLE NOT NULL,
  delta_minutes BIGINT NOT NULL DEFAULT 0,
  delta_temperature_median DOUBLE,
  power_minutes BIGINT NOT NULL DEFAULT 0,
  cpu_power_median DOUBLE,
  power_fit_n BIGINT NOT NULL DEFAULT 0,
  power_fit_sum_x DOUBLE NOT NULL DEFAULT 0,
  power_fit_sum_y DOUBLE NOT NULL DEFAULT 0,
  power_fit_sum_xy DOUBLE NOT NULL DEFAULT 0,
  power_fit_sum_xx DOUBLE NOT NULL DEFAULT 0,
  power_fit_sum_yy DOUBLE NOT NULL DEFAULT 0,
  PRIMARY KEY (date, source, band)
);

CREATE TABLE cooling_fan_covariate_daily_summary (
  date VARCHAR NOT NULL,
  source VARCHAR NOT NULL,
  fan_source VARCHAR NOT NULL,
  band VARCHAR NOT NULL,
  rpm_minutes BIGINT NOT NULL,
  rpm_median DOUBLE NOT NULL,
  fit_n BIGINT NOT NULL DEFAULT 0,
  fit_sum_x DOUBLE NOT NULL DEFAULT 0,
  fit_sum_y DOUBLE NOT NULL DEFAULT 0,
  fit_sum_xy DOUBLE NOT NULL DEFAULT 0,
  fit_sum_xx DOUBLE NOT NULL DEFAULT 0,
  fit_sum_yy DOUBLE NOT NULL DEFAULT 0,
  PRIMARY KEY (date, source, fan_source, band)
);
"#;

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn stable_schema_names_every_current_domain_table_once() {
    assert_eq!(TABLES.len(), 15);
    for table in TABLES {
      assert_eq!(
        NATIVE_SCHEMA_SQL
          .matches(&format!("CREATE TABLE {table} "))
          .count(),
        1,
        "{table}"
      );
    }
    assert_eq!(
      NATIVE_SCHEMA_SQL.matches("CREATE TABLE ").count(),
      TABLES.len()
    );
  }

  #[test]
  fn storage_devices_is_created_before_the_table_referencing_it() {
    let parent = TABLES.iter().position(|t| *t == "storage_devices").unwrap();
    let child = TABLES
      .iter()
      .position(|t| *t == "storage_health_daily_records")
      .unwrap();
    assert!(parent < child);
  }

  #[test]
  fn only_writer_proven_mixed_measurements_use_tagged_unions() {
    assert_eq!(
      NATIVE_SCHEMA_SQL
        .matches("UNION(i BIGINT, r DOUBLE)")
        .count(),
      10
    );
  }

  #[test]
  fn derived_epoch_columns_stay_nullable_so_an_unconvertible_stamp_reads_absent() {
    assert_eq!(TIMESTAMPS.len(), 4);
    assert_eq!(
      NATIVE_SCHEMA_SQL
        .matches("__hv_timestamp_epoch_ms BIGINT\n")
        .count(),
      4
    );
    assert!(!NATIVE_SCHEMA_SQL.contains("__hv_timestamp_epoch_ms BIGINT NOT NULL"));
  }

  #[test]
  fn mutable_domain_keys_have_native_constraints() {
    for constraint in [
      "UNIQUE(device_id, date)",
      "PRIMARY KEY (date, source)",
      "PRIMARY KEY (date, source, band)",
      "PRIMARY KEY (date, source, fan_source, band)",
      "CHECK (id = 1)",
    ] {
      assert!(
        NATIVE_SCHEMA_SQL.contains(constraint),
        "missing {constraint}"
      );
    }
  }

  #[test]
  fn every_identity_names_a_declared_table() {
    assert_eq!(IDENTITIES.len(), 6);
    for identity in IDENTITIES {
      assert!(TABLES.contains(&identity.table), "{}", identity.table);
    }
    let autoincrement = IDENTITIES
      .iter()
      .filter(|identity| {
        matches!(identity.mode, NativeIdentityMode::AutoIncrement { .. })
      })
      .count();
    assert_eq!(autoincrement, 3);
  }
}
