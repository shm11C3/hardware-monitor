#![cfg(feature = "duckdb-archive")]
//! The Cooling Insight projections, native beside SQLite.
//!
//! Same differential discipline as `duckdb_ambient_fan`: one fixture seeded
//! through the real SQLite path, finalized through the real finalizer, and
//! then the same question put to both engines. The rollup's own six-table
//! transaction is tested on writable copies, because proving a boundary needs
//! a write that fails.

mod native_support;

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Utc};
use hardviz_core::infrastructure::database::native_database::{
  DayRollup, NativeCancellation, NativeDatabase, NativeDatabaseError,
  NativeDatabaseOptions, cooling_baseline as native_baseline,
  cooling_covariate_daily_summary as native_covariate,
  cooling_daily_summary as native_daily, cooling_delta_baseline as native_delta_baseline,
  cooling_fan_daily_summary as native_fan_daily, cooling_hourly_summary as native_hourly,
  cooling_rollup as native_rollup,
  cooling_thermal_delta_daily_summary as native_thermal_delta,
};
use hardviz_core::infrastructure::database::{
  ambient_archive, cooling_baseline, cooling_covariate_daily_summary,
  cooling_daily_summary, cooling_delta_baseline, cooling_fan_daily_summary,
  cooling_hourly_summary, cooling_thermal_delta_daily_summary, db, hardware_archive,
};
use hardviz_core::persistence::archive_data::{
  AmbientData, HardwareArchiveRow, HardwareData,
};
use hardviz_core::persistence::cooling_baseline::EstablishedBaseline;
use hardviz_core::persistence::cooling_covariate_rollup::{
  CovariateDailySummary, CovariateDaySummary, FanCovariateDailySummary,
  PairedFitStatistics,
};
use hardviz_core::persistence::cooling_fan_rollup::FanDailySummary;
use hardviz_core::persistence::cooling_hourly_rollup::HourlyCoolingSummary;
use hardviz_core::persistence::cooling_rollup::{
  BandSummary, CpuLoadBand, DailyCoolingSummary, PowerSummary,
};
use hardviz_core::persistence::cooling_thermal_delta_rollup::ThermalDeltaDailySummary;
use native_support::{NativeFixture, app_native_schema};
use sqlx::SqlitePool;
use tokio::sync::OnceCell;

fn at(text: &str) -> DateTime<Utc> {
  text.parse().unwrap()
}

fn day(text: &str) -> NaiveDate {
  text.parse().unwrap()
}

fn hour(text: &str) -> NaiveDateTime {
  NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").unwrap()
}

fn cancel() -> NativeCancellation {
  NativeCancellation::new()
}

fn reading(avg: f32) -> HardwareData {
  HardwareData {
    avg: Some(avg),
    max: Some(avg + 1.0),
    min: Some(avg - 1.0),
  }
}

fn absent() -> HardwareData {
  HardwareData {
    avg: None,
    max: None,
    min: None,
  }
}

fn band(avg: f32, minutes: u32) -> BandSummary {
  BandSummary {
    avg: Some(avg),
    max: Some(avg + 2.0),
    min: Some(avg - 2.0),
    sample_minutes: minutes,
  }
}

/// A band whose every reading is NaN. SQLite stores each of them as NULL, so
/// this is indistinguishable from [`empty_band`] once written - which is
/// exactly the equivalence the native writer has to reproduce.
fn nan_band() -> BandSummary {
  BandSummary {
    avg: Some(f32::NAN),
    max: Some(f32::NAN),
    min: Some(f32::NAN),
    sample_minutes: 0,
  }
}

fn empty_band() -> BandSummary {
  BandSummary {
    avg: None,
    max: None,
    min: None,
    sample_minutes: 0,
  }
}

fn fit(n: u32) -> PairedFitStatistics {
  PairedFitStatistics {
    n,
    sum_x: 1.5,
    sum_y: 2.5,
    sum_xy: 3.5,
    sum_xx: 4.5,
    sum_yy: 5.5,
  }
}

/// The seeded SQLite source and the finalized template copied from it.
///
/// Deliberately does *not* hold an open `NativeDatabase`: see [`Owned`].
struct Shared {
  fixture: NativeFixture,
}

static SHARED: OnceCell<Shared> = OnceCell::const_new();

async fn shared() -> &'static Shared {
  SHARED
    .get_or_init(|| async {
      let fixture = NativeFixture::new();
      assert!(db::init(fixture.source.clone()));
      let pool = fixture.migrated_pool().await;
      seed_archives().await;
      seed_projections(&pool).await;
      pool.close().await;

      fixture.finalize().await;
      Shared { fixture }
    })
    .await
}

/// A private copy of the finalized fixture, plus the owner serving it.
///
/// Every test takes its own copy instead of sharing one open `NativeDatabase`.
/// On Windows a DuckDB file cannot be read, hashed, or opened by a second
/// instance while any owner still holds it, so a shared open handle makes
/// `std::fs::copy` fail with a sharing violation and `read_only` fail with
/// "File is already open". Finalization - the expensive step - stays shared;
/// only the copy is per test.
struct Owned {
  /// Kept alive so [`Owned::path`] stays valid after the database is closed.
  _directory: tempfile::TempDir,
  path: std::path::PathBuf,
  database: Option<NativeDatabase>,
}

impl Owned {
  async fn open(name: &str) -> Self {
    let shared = shared().await;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(name);
    // Safe because nothing holds the finalized template: `shared` deliberately
    // does not keep it open.
    std::fs::copy(&shared.fixture.finalized, &path).unwrap();
    let database = NativeDatabase::open(
      &path,
      NativeDatabaseOptions::new(app_native_schema::NATIVE_SCHEMA_VERSION),
    )
    .await
    .unwrap();
    Self {
      _directory: directory,
      path,
      database: Some(database),
    }
  }

  fn database(&self) -> &NativeDatabase {
    self.database.as_ref().expect("database already closed")
  }

  /// Release the file so a second instance can open it. Call before any
  /// `read_only` on [`Owned::path`].
  async fn close(&mut self) {
    if let Some(database) = self.database.take() {
      database.close().await.unwrap();
    }
  }
}

/// Archive minutes shaped so every branch of the reads has something to do:
/// a minute with no CPU usage (unclassifiable), a minute with no CPU
/// temperature, a minute with no power triple, and an ambient source that
/// stops reporting part way through.
async fn seed_archives() {
  let base = at("2026-09-01T00:01:00Z");
  for minute in 0..12i64 {
    let tick = base + Duration::minutes(minute);
    hardware_archive::insert(
      HardwareArchiveRow {
        // Minute 4 carries no CPU usage, so it pairs but cannot be
        // classified into a load band.
        cpu: if minute == 4 {
          absent()
        } else {
          reading(10.0 + minute as f32 * 7.0)
        },
        memory: reading(40.0),
        cpu_temperature: if minute == 7 {
          absent()
        } else {
          reading(55.0 + minute as f32 * 0.5)
        },
        // Minute 9 has only a partial power triple, which the power gate
        // must refuse on both sides.
        cpu_power: if minute == 9 {
          HardwareData {
            avg: Some(12.0),
            max: None,
            min: None,
          }
        } else {
          reading(12.0 + minute as f32)
        },
        gpu_power: absent(),
        ane_power: absent(),
        package_power: absent(),
      },
      tick,
    )
    .await
    .unwrap();

    // Ambient only for the first eight minutes, and the second source only
    // for the first four.
    if minute < 8 {
      let mut rows = vec![AmbientData {
        source: "Room".to_owned(),
        temperature: 21.0 + minute as f32 * 0.25,
        humidity: Some(48.0),
      }];
      if minute < 4 {
        rows.push(AmbientData {
          source: "Cabinet".to_owned(),
          temperature: 24.5,
          humidity: None,
        });
      }
      ambient_archive::insert(rows, tick).await.unwrap();
    }
  }

  // An ambient row with no hardware row in its minute: it must never count
  // as coverage, however often the day is re-rolled.
  ambient_archive::insert(
    vec![AmbientData {
      source: "Room".to_owned(),
      temperature: 22.0,
      humidity: None,
    }],
    at("2026-09-01T06:30:00Z"),
  )
  .await
  .unwrap();
}

/// The five summary tables. `cooling_daily_summary` and
/// `cooling_hourly_summary` go through their own public writers; the other
/// three have no public single-row writer, so they are seeded with the same
/// column set their `upsert_with` writes. Provenance does not matter here -
/// these rows exist so two *readers* can be compared over them.
async fn seed_projections(pool: &SqlitePool) {
  for (date, coverage) in [("2026-08-28", 900u32), ("2026-08-29", 1200)] {
    cooling_daily_summary::upsert(&DailyCoolingSummary {
      date: day(date),
      coverage_minutes: coverage,
      idle: band(41.0, 300),
      low: band(48.5, 240),
      // A band with no samples has to come back as absent rather than as a
      // measured zero.
      mid: empty_band(),
      high: band(72.25, 120),
      power: PowerSummary {
        avg: Some(13.5),
        max: Some(28.0),
        min: Some(4.25),
        sample_minutes: 660,
      },
    })
    .await
    .unwrap();
  }
  // A day whose every reading is NaN. SQLite has no NaN: the writer's bind is
  // stored as NULL, so the day reads back as one with no readings at all. It
  // is seeded here rather than in a test of its own so that every whole-table
  // comparison below covers it - a native writer that stored the NaN instead
  // would make all of them disagree.
  cooling_daily_summary::upsert(&DailyCoolingSummary {
    date: day("2026-08-31"),
    coverage_minutes: 0,
    idle: nan_band(),
    low: nan_band(),
    mid: nan_band(),
    high: nan_band(),
    power: PowerSummary {
      avg: Some(f32::NAN),
      max: Some(f32::NAN),
      min: Some(f32::NAN),
      sample_minutes: 0,
    },
  })
  .await
  .unwrap();
  cooling_hourly_summary::upsert(&HourlyCoolingSummary {
    hour_start: hour("2026-08-31 00:00:00"),
    cpu_usage_avg: Some(f32::NAN),
    cpu_temperature_avg: Some(f32::NAN),
    sample_minutes: 0,
  })
  .await
  .unwrap();

  // A day whose bands are all empty: the pairable cursor must skip it while
  // `max_summarized_date` still sees it.
  cooling_daily_summary::upsert(&DailyCoolingSummary {
    date: day("2026-08-30"),
    coverage_minutes: 0,
    idle: empty_band(),
    low: empty_band(),
    mid: empty_band(),
    high: empty_band(),
    power: PowerSummary {
      avg: None,
      max: None,
      min: None,
      sample_minutes: 0,
    },
  })
  .await
  .unwrap();

  for offset in 0..4u32 {
    cooling_hourly_summary::upsert(&HourlyCoolingSummary {
      hour_start: hour(&format!("2026-08-28 {:02}:00:00", offset * 6)),
      cpu_usage_avg: Some(12.5 + offset as f32),
      cpu_temperature_avg: Some(50.0 + offset as f32),
      sample_minutes: 60 - offset,
    })
    .await
    .unwrap();
  }
  // An `hour_start` no format can read: both readers must drop that one row
  // rather than fail the query.
  sqlx::query(
    "INSERT INTO cooling_hourly_summary (hour_start, cpu_usage_avg, cpu_temperature_avg, sample_minutes)
     VALUES ('2026-08-28 not-an-hour', 1.0, 2.0, 3)",
  )
  .execute(pool)
  .await
  .unwrap();

  for (date, source, rpm_avg, rpm_max, rpm_min, minutes) in [
    ("2026-08-28", "Exhaust", 1234.5f64, 1800i64, 900i64, 600i64),
    ("2026-08-28", "Intake", 0.0, 0, 0, 600),
    ("2026-08-29", "Exhaust", 1300.0, 1900, 1000, 720),
  ] {
    sqlx::query(
      "INSERT INTO cooling_fan_daily_summary (date, source, rpm_avg, rpm_max, rpm_min, sample_minutes)
       VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(date)
    .bind(source)
    .bind(rpm_avg)
    .bind(rpm_max)
    .bind(rpm_min)
    .bind(minutes)
    .execute(pool)
    .await
    .unwrap();
  }

  for (date, source) in [
    ("2026-08-28", "Room"),
    ("2026-08-28", "Cabinet"),
    ("2026-08-29", "Room"),
  ] {
    sqlx::query(
      "INSERT INTO cooling_thermal_delta_daily_summary (
         date, source, coverage_minutes,
         idle_delta_temperature_avg, idle_delta_temperature_max, idle_delta_temperature_min, idle_delta_sample_minutes,
         low_delta_temperature_avg, low_delta_temperature_max, low_delta_temperature_min, low_delta_sample_minutes,
         mid_delta_temperature_avg, mid_delta_temperature_max, mid_delta_temperature_min, mid_delta_sample_minutes,
         high_delta_temperature_avg, high_delta_temperature_max, high_delta_temperature_min, high_delta_sample_minutes)
       VALUES ($1, $2, 480, 18.5, 20.0, 17.0, 200, 24.0, 26.0, 22.0, 150, NULL, NULL, NULL, 0, 40.5, 44.0, 38.0, 130)",
    )
    .bind(date)
    .bind(source)
    .execute(pool)
    .await
    .unwrap();
  }

  for (date, source, band_key, delta_median, power_median) in [
    ("2026-08-28", "Room", "idle", Some(18.5f64), Some(9.0f64)),
    ("2026-08-28", "Room", "high", Some(40.5), None),
    ("2026-08-28", "Cabinet", "mid", None, Some(15.0)),
    ("2026-08-29", "Room", "low", Some(24.0), Some(11.0)),
  ] {
    sqlx::query(
      "INSERT INTO cooling_covariate_daily_summary (
         date, source, band, sample_minutes, band_share, ambient_temperature_median,
         delta_minutes, delta_temperature_median, power_minutes, cpu_power_median,
         power_fit_n, power_fit_sum_x, power_fit_sum_y, power_fit_sum_xy, power_fit_sum_xx, power_fit_sum_yy)
       VALUES ($1, $2, $3, 240, 0.25, 21.5, 200, $4, 180, $5, 7, 1.5, 2.5, 3.5, 4.5, 5.5)",
    )
    .bind(date)
    .bind(source)
    .bind(band_key)
    .bind(delta_median)
    .bind(power_median)
    .execute(pool)
    .await
    .unwrap();
  }

  for (date, source, fan_source, band_key) in [
    ("2026-08-28", "Room", "Exhaust", "idle"),
    ("2026-08-28", "Room", "Intake", "high"),
    ("2026-08-29", "Room", "Exhaust", "low"),
  ] {
    sqlx::query(
      "INSERT INTO cooling_fan_covariate_daily_summary (
         date, source, fan_source, band, rpm_minutes, rpm_median,
         fit_n, fit_sum_x, fit_sum_y, fit_sum_xy, fit_sum_xx, fit_sum_yy)
       VALUES ($1, $2, $3, $4, 210, 1150.5, 9, 1.5, 2.5, 3.5, 4.5, 5.5)",
    )
    .bind(date)
    .bind(source)
    .bind(fan_source)
    .bind(band_key)
    .execute(pool)
    .await
    .unwrap();
  }

  cooling_baseline::insert_established_baseline(&EstablishedBaseline {
    idle_temperature_avg: 41.25,
    window_start_date: day("2026-08-28"),
    window_end_date: day("2026-08-29"),
    sample_minutes: 540,
  })
  .await
  .unwrap();

  sqlx::query(
    "INSERT INTO cooling_delta_baseline
       (id, source, window_start_date, window_end_date, delta_temperature_avg, sample_minutes, established_at)
     VALUES (1, 'Room', '2026-08-28', '2026-08-29', 18.75, 480, '2026-08-30T00:00:00+00:00')",
  )
  .execute(pool)
  .await
  .unwrap();
}

#[tokio::test]
async fn archive_reads_answer_what_sqlite_answers() {
  let mut owned = Owned::open("archive-reads.duckdb").await;
  let (start, end) = (at("2026-09-01T00:00:00Z"), at("2026-09-02T00:00:00Z"));

  let native = native_daily::select_archive_minutes_for_range(
    owned.database(),
    cancel(),
    &start,
    &end,
  )
  .await
  .unwrap();
  let sqlite = cooling_daily_summary::select_archive_minutes_for_range(&start, &end)
    .await
    .unwrap();
  assert_eq!(native, sqlite);
  assert_eq!(native.len(), 12);
  assert!(native.iter().any(|minute| minute.cpu_usage_avg.is_none()));
  assert!(
    native
      .iter()
      .any(|minute| minute.cpu_temperature_avg.is_none())
  );

  assert_eq!(
    native_daily::earliest_archived_timestamp(owned.database(), cancel())
      .await
      .unwrap(),
    cooling_daily_summary::earliest_archived_timestamp()
      .await
      .unwrap()
  );

  for before in [
    at("2026-09-01T00:06:00Z"),
    at("2026-09-01T00:11:00Z"),
    at("2026-09-02T00:00:00Z"),
    at("2020-01-01T00:00:00Z"),
  ] {
    assert_eq!(
      native_daily::max_powered_archive_timestamp_before(
        owned.database(),
        cancel(),
        &before
      )
      .await
      .unwrap(),
      cooling_daily_summary::max_powered_archive_timestamp_before(&before)
        .await
        .unwrap(),
      "before {before}"
    );
  }
  owned.close().await;
}

#[tokio::test]
async fn paired_minute_reads_and_cursors_answer_what_sqlite_answers() {
  let mut owned = Owned::open("paired-minutes.duckdb").await;
  let (start, end) = (at("2026-09-01T00:00:00Z"), at("2026-09-02T00:00:00Z"));

  let native = native_thermal_delta::select_thermal_delta_minutes_for_range(
    owned.database(),
    cancel(),
    &start,
    &end,
  )
  .await
  .unwrap();
  let sqlite =
    cooling_thermal_delta_daily_summary::select_thermal_delta_minutes_for_range(
      &start, &end,
    )
    .await
    .unwrap();
  assert_eq!(native, sqlite);
  // Four minutes with two sources plus four with one: twelve pairs, and none
  // for the ambient row whose minute has no hardware row.
  assert_eq!(native.len(), 12);
  assert!(native.iter().any(|pair| pair.source == "Cabinet"));

  for before in [
    at("2026-09-01T00:03:00Z"),
    at("2026-09-01T00:09:00Z"),
    at("2026-09-02T00:00:00Z"),
    at("2020-01-01T00:00:00Z"),
  ] {
    assert_eq!(
      native_thermal_delta::max_pairable_ambient_archive_timestamp_before(
        owned.database(),
        cancel(),
        &before
      )
      .await
      .unwrap(),
      cooling_thermal_delta_daily_summary::max_pairable_ambient_archive_timestamp_before(
        &before
      )
      .await
      .unwrap(),
      "pairable, before {before}"
    );
    assert_eq!(
      native_covariate::max_classifiable_pairable_ambient_archive_timestamp_before(
        owned.database(),
        cancel(),
        &before
      )
      .await
      .unwrap(),
      cooling_covariate_daily_summary::max_classifiable_pairable_ambient_archive_timestamp_before(
        &before
      )
      .await
      .unwrap(),
      "classifiable, before {before}"
    );
  }

  // The unclassifiable minute is what makes the two cursors different
  // questions, so the fixture must actually separate them.
  let pairable =
    cooling_thermal_delta_daily_summary::max_pairable_ambient_archive_timestamp_before(
      &at("2026-09-01T00:06:00Z"),
    )
    .await
    .unwrap();
  let classifiable =
    cooling_covariate_daily_summary::max_classifiable_pairable_ambient_archive_timestamp_before(
      &at("2026-09-01T00:06:00Z"),
    )
    .await
    .unwrap();
  assert_ne!(pairable, classifiable);
  owned.close().await;
}

#[tokio::test]
async fn every_projection_reads_what_sqlite_reads() {
  let mut owned = Owned::open("projections.duckdb").await;

  assert_eq!(
    native_daily::select_all_daily_cooling_summaries(owned.database(), cancel())
      .await
      .unwrap(),
    cooling_daily_summary::select_all_daily_cooling_summaries()
      .await
      .unwrap()
  );
  assert_eq!(
    native_daily::select_daily_idle_samples(owned.database(), cancel())
      .await
      .unwrap(),
    cooling_daily_summary::select_daily_idle_samples()
      .await
      .unwrap()
  );
  assert_eq!(
    native_fan_daily::select_all_fan_daily_summaries(owned.database(), cancel())
      .await
      .unwrap(),
    cooling_fan_daily_summary::select_all_fan_daily_summaries()
      .await
      .unwrap()
  );
  assert_eq!(
    native_thermal_delta::select_all_thermal_delta_daily_summaries(
      owned.database(),
      cancel()
    )
    .await
    .unwrap(),
    cooling_thermal_delta_daily_summary::select_all_thermal_delta_daily_summaries()
      .await
      .unwrap()
  );
  assert_eq!(
    native_covariate::select_all_covariate_daily_summaries(owned.database(), cancel())
      .await
      .unwrap(),
    cooling_covariate_daily_summary::select_all_covariate_daily_summaries()
      .await
      .unwrap()
  );
  assert_eq!(
    native_covariate::select_all_fan_covariate_daily_summaries(
      owned.database(),
      cancel()
    )
    .await
    .unwrap(),
    cooling_covariate_daily_summary::select_all_fan_covariate_daily_summaries()
      .await
      .unwrap()
  );

  // The hourly read drops the unreadable `hour_start` on both sides rather
  // than failing, and keeps the four real hours.
  let hours = native_hourly::select_hours_in_date_range(
    owned.database(),
    cancel(),
    day("2026-08-28"),
    day("2026-08-29"),
  )
  .await
  .unwrap();
  assert_eq!(
    hours,
    cooling_hourly_summary::select_hours_in_date_range(
      day("2026-08-28"),
      day("2026-08-29")
    )
    .await
    .unwrap()
  );
  assert_eq!(hours.len(), 4);

  assert_eq!(
    native_baseline::select_established_baseline(owned.database(), cancel())
      .await
      .unwrap(),
    cooling_baseline::select_established_baseline()
      .await
      .unwrap()
  );
  assert_eq!(
    native_delta_baseline::select_established_delta_baseline(owned.database(), cancel())
      .await
      .unwrap(),
    cooling_delta_baseline::select_established_delta_baseline()
      .await
      .unwrap()
  );
  owned.close().await;
}

#[tokio::test]
async fn every_cursor_answers_what_sqlite_answers() {
  let mut owned = Owned::open("cursors.duckdb").await;

  assert_eq!(
    native_daily::max_summarized_date(owned.database(), cancel())
      .await
      .unwrap(),
    cooling_daily_summary::max_summarized_date().await.unwrap()
  );
  assert_eq!(
    native_daily::max_pairable_summarized_date(owned.database(), cancel())
      .await
      .unwrap(),
    cooling_daily_summary::max_pairable_summarized_date()
      .await
      .unwrap()
  );
  assert_eq!(
    native_daily::max_powered_summarized_date(owned.database(), cancel())
      .await
      .unwrap(),
    cooling_daily_summary::max_powered_summarized_date()
      .await
      .unwrap()
  );
  assert_eq!(
    native_hourly::max_summarized_date(owned.database(), cancel())
      .await
      .unwrap(),
    cooling_hourly_summary::max_summarized_date().await.unwrap()
  );
  assert_eq!(
    native_fan_daily::max_summarized_date(owned.database(), cancel())
      .await
      .unwrap(),
    cooling_fan_daily_summary::max_summarized_date()
      .await
      .unwrap()
  );
  assert_eq!(
    native_thermal_delta::max_summarized_date(owned.database(), cancel())
      .await
      .unwrap(),
    cooling_thermal_delta_daily_summary::max_summarized_date()
      .await
      .unwrap()
  );
  assert_eq!(
    native_covariate::max_summarized_date(owned.database(), cancel())
      .await
      .unwrap(),
    cooling_covariate_daily_summary::max_summarized_date()
      .await
      .unwrap()
  );

  // The empty-band day is what separates `max_summarized_date` from
  // `max_pairable_summarized_date`; without it the two cursors would agree by
  // accident and the comparison above would prove nothing.
  assert_eq!(
    cooling_daily_summary::max_summarized_date().await.unwrap(),
    Some(day("2026-08-31"))
  );
  assert_eq!(
    cooling_daily_summary::max_pairable_summarized_date()
      .await
      .unwrap(),
    Some(day("2026-08-29"))
  );
  owned.close().await;
}

fn one_days_rollup(date: NaiveDate) -> DayRollup {
  DayRollup {
    summary: Some(DailyCoolingSummary {
      date,
      coverage_minutes: 1440,
      idle: band(40.0, 400),
      low: band(50.0, 400),
      mid: band(60.0, 400),
      high: band(70.0, 240),
      power: PowerSummary {
        avg: Some(14.0),
        max: Some(30.0),
        min: Some(5.0),
        sample_minutes: 1440,
      },
    }),
    hours: vec![
      HourlyCoolingSummary {
        hour_start: hour("2026-09-05 00:00:00"),
        cpu_usage_avg: Some(11.0),
        cpu_temperature_avg: Some(48.0),
        sample_minutes: 60,
      },
      HourlyCoolingSummary {
        hour_start: hour("2026-09-05 01:00:00"),
        cpu_usage_avg: None,
        cpu_temperature_avg: Some(49.0),
        sample_minutes: 55,
      },
    ],
    fans: vec![FanDailySummary {
      date,
      source: "Exhaust".to_owned(),
      rpm_avg: 1100.5,
      rpm_max: 1800,
      rpm_min: 700,
      sample_minutes: 1400,
    }],
    thermal_deltas: vec![ThermalDeltaDailySummary {
      date,
      source: "Room".to_owned(),
      coverage_minutes: 900,
      idle: band(18.0, 300),
      low: band(24.0, 300),
      mid: empty_band(),
      high: band(40.0, 300),
    }],
    covariates: CovariateDaySummary {
      bands: vec![CovariateDailySummary {
        date,
        source: "Room".to_owned(),
        band: CpuLoadBand::Idle,
        sample_minutes: 300,
        band_share: 0.25,
        ambient_temperature_median: 21.5,
        delta_minutes: 280,
        delta_temperature_median: Some(18.0),
        power_minutes: 260,
        cpu_power_median: Some(9.5),
        delta_per_watt: fit(7),
      }],
      fans: vec![FanCovariateDailySummary {
        date,
        source: "Room".to_owned(),
        fan_source: "Exhaust".to_owned(),
        band: CpuLoadBand::Idle,
        rpm_minutes: 250,
        rpm_median: 1120.0,
        delta_per_rpm: fit(5),
      }],
    },
  }
}

/// One day, six tables, one commit - and the rows read back exactly as
/// written, through the same readers the projections use.
#[tokio::test]
async fn one_days_rollup_commits_all_six_projections_together() {
  let mut owned = Owned::open("rollup.duckdb").await;
  let native = owned.database();
  let date = day("2026-09-05");
  let rollup = one_days_rollup(date);

  native_rollup::persist_day_rollup(native, cancel(), one_days_rollup(date))
    .await
    .unwrap();

  let daily = native_daily::select_all_daily_cooling_summaries(native, cancel())
    .await
    .unwrap();
  assert!(daily.contains(rollup.summary.as_ref().unwrap()));
  let hours = native_hourly::select_hours_in_date_range(native, cancel(), date, date)
    .await
    .unwrap();
  assert_eq!(hours, rollup.hours);
  assert!(
    native_fan_daily::select_all_fan_daily_summaries(native, cancel())
      .await
      .unwrap()
      .contains(&rollup.fans[0])
  );
  assert!(
    native_thermal_delta::select_all_thermal_delta_daily_summaries(native, cancel())
      .await
      .unwrap()
      .contains(&rollup.thermal_deltas[0])
  );
  assert!(
    native_covariate::select_all_covariate_daily_summaries(native, cancel())
      .await
      .unwrap()
      .contains(&rollup.covariates.bands[0])
  );
  assert!(
    native_covariate::select_all_fan_covariate_daily_summaries(native, cancel())
      .await
      .unwrap()
      .contains(&rollup.covariates.fans[0])
  );

  // Re-rolling the same day updates in place rather than duplicating: the
  // rollup is idempotent because the catch-up cursor may retry a day.
  native_rollup::persist_day_rollup(native, cancel(), one_days_rollup(date))
    .await
    .unwrap();
  assert_eq!(
    native_daily::select_all_daily_cooling_summaries(native, cancel())
      .await
      .unwrap()
      .iter()
      .filter(|summary| summary.date == date)
      .count(),
    1
  );
  owned.close().await;
}

/// A day that fails part way through leaves *nothing* behind. A committed
/// daily row with its later projections missing is the half-written state the
/// catch-up cursor cannot tell apart from a day that legitimately had none, so
/// the whole day has to roll back and be retried.
///
/// The failure is induced by removing the last table the transaction writes,
/// which is the only way to make the sixth statement fail while the first five
/// succeed.
#[tokio::test]
async fn a_day_that_fails_part_way_through_leaves_nothing_behind() {
  let mut owned = Owned::open("rollback.duckdb").await;
  // Released before a raw connection touches the file: on Windows a second
  // DuckDB instance cannot open it while the owner still holds it.
  owned.close().await;
  let path = owned.path.clone();
  duckdb::Connection::open(&path)
    .unwrap()
    .execute_batch("DROP TABLE cooling_fan_covariate_daily_summary")
    .unwrap();

  let native = NativeDatabase::open(
    &path,
    NativeDatabaseOptions::new(app_native_schema::NATIVE_SCHEMA_VERSION),
  )
  .await
  .unwrap();
  let date = day("2026-09-05");
  let error = native_rollup::persist_day_rollup(&native, cancel(), one_days_rollup(date))
    .await
    .unwrap_err();
  assert!(
    matches!(error, NativeDatabaseError::DuckDb { .. }),
    "{error:?}"
  );

  // None of the five tables that did accept their statement kept the row.
  assert!(
    !native_daily::select_all_daily_cooling_summaries(&native, cancel())
      .await
      .unwrap()
      .iter()
      .any(|summary| summary.date == date)
  );
  assert!(
    native_hourly::select_hours_in_date_range(&native, cancel(), date, date)
      .await
      .unwrap()
      .is_empty()
  );
  for present in [
    native_fan_daily::select_all_fan_daily_summaries(&native, cancel())
      .await
      .unwrap()
      .iter()
      .any(|fan| fan.date == date),
    native_thermal_delta::select_all_thermal_delta_daily_summaries(&native, cancel())
      .await
      .unwrap()
      .iter()
      .any(|delta| delta.date == date),
    native_covariate::select_all_covariate_daily_summaries(&native, cancel())
      .await
      .unwrap()
      .iter()
      .any(|covariate| covariate.date == date),
  ] {
    assert!(!present);
  }
  // The reopened owner, not `owned`, is what holds the file now.
  native.close().await.unwrap();
}

/// The pinned baselines are write-once. A second establishment must change
/// nothing - not even `established_at` - because replacing the pinned value is
/// exactly the drift the table exists to prevent.
#[tokio::test]
async fn pinning_a_baseline_twice_changes_nothing() {
  let mut owned = Owned::open("baseline.duckdb").await;
  let native = owned.database();
  let before = native_baseline::select_established_baseline(native, cancel())
    .await
    .unwrap()
    .unwrap();

  native_baseline::insert_established_baseline(
    native,
    cancel(),
    &EstablishedBaseline {
      idle_temperature_avg: 99.0,
      window_start_date: day("2026-01-01"),
      window_end_date: day("2026-01-07"),
      sample_minutes: 1,
    },
    at("2027-01-01T00:00:00Z"),
  )
  .await
  .unwrap();

  assert_eq!(
    native_baseline::select_established_baseline(native, cancel())
      .await
      .unwrap(),
    Some(before)
  );
  owned.close().await;

  let connection = native_support::read_only(&owned.path);
  let established_at: String = connection
    .query_row(
      "SELECT established_at FROM cooling_baseline WHERE id = 1",
      [],
      |row| row.get(0),
    )
    .unwrap();
  assert!(
    !established_at.starts_with("2027"),
    "the second establishment must not touch established_at, got {established_at}"
  );
}

/// Retention takes the same rows on both sides, and a preserved window is
/// exempt on both sides. The cutoff is a local date in both engines, so a
/// zero-day retention takes everything the window does not protect.
#[tokio::test]
async fn retention_preserves_the_pinned_window_the_way_sqlite_does() {
  let mut owned = Owned::open("retention.duckdb").await;
  let native = owned.database();
  let window = [(day("2026-08-29"), day("2026-08-29"))];

  native_daily::delete_old_data(native, cancel(), 0, &window)
    .await
    .unwrap();
  assert_eq!(
    native_daily::select_all_daily_cooling_summaries(native, cancel())
      .await
      .unwrap()
      .into_iter()
      .map(|summary| summary.date)
      .collect::<Vec<_>>(),
    vec![day("2026-08-29")],
    "only the preserved window survives a zero-day retention"
  );

  // The hourly exemption has to cover the protected day's 23:00 row, which is
  // why its upper bound is suffixed past any hour a day can carry.
  native_hourly::delete_old_data(
    native,
    cancel(),
    0,
    &[(day("2026-08-28"), day("2026-08-28"))],
  )
  .await
  .unwrap();
  assert_eq!(
    native_hourly::select_hours_in_date_range(
      native,
      cancel(),
      day("2026-08-28"),
      day("2026-08-28")
    )
    .await
    .unwrap()
    .len(),
    4,
    "every hour of the protected day survives"
  );

  // The fan projection has no protected window: nothing is derived from it.
  native_fan_daily::delete_old_data(native, cancel(), 0)
    .await
    .unwrap();
  assert!(
    native_fan_daily::select_all_fan_daily_summaries(native, cancel())
      .await
      .unwrap()
      .is_empty()
  );

  // Both co-variate tables age out on one cutoff: a band row whose fan rows
  // had aged out from under it would be a projection half-present.
  native_covariate::delete_old_data(native, cancel(), 0, &window)
    .await
    .unwrap();
  let bands = native_covariate::select_all_covariate_daily_summaries(native, cancel())
    .await
    .unwrap();
  let fans = native_covariate::select_all_fan_covariate_daily_summaries(native, cancel())
    .await
    .unwrap();
  assert!(bands.iter().all(|row| row.date == day("2026-08-29")));
  assert!(fans.iter().all(|row| row.date == day("2026-08-29")));
  assert!(!bands.is_empty() && !fans.is_empty());

  native_thermal_delta::delete_old_data(native, cancel(), 0, &window)
    .await
    .unwrap();
  let deltas =
    native_thermal_delta::select_all_thermal_delta_daily_summaries(native, cancel())
      .await
      .unwrap();
  // `all` over an empty collection is vacuously true, so the survivors have to
  // be shown to exist before their dates mean anything - a delete that took
  // the protected window too would otherwise pass this assertion.
  assert!(!deltas.is_empty(), "the preserved window must survive");
  assert!(deltas.iter().all(|row| row.date == day("2026-08-29")));
  owned.close().await;
}

/// SQLite has no NaN. Its writer's bind is stored as NULL, so a day of NaN
/// readings is indistinguishable from a day with no readings - and the native
/// writer has to reach the same state, or every whole-table comparison in this
/// file would disagree on that day.
///
/// The seeded NaN day is what makes those comparisons cover this; what is
/// checked here is the two halves they cannot show on their own: that SQLite
/// really stored NULL rather than a NaN, so the equivalence is measured rather
/// than assumed, and that the *native* writer given the same NaN reaches that
/// same NULL instead of storing a NaN a later `AVG` would propagate.
#[tokio::test]
async fn a_nan_summary_reading_is_the_same_absence_in_both_engines() {
  let shared = shared().await;

  let pool = native_support::open_pool(&shared.fixture.source, false).await;
  let stored: Vec<String> = sqlx::query_scalar(
    "SELECT typeof(idle_cpu_temperature_avg) FROM cooling_daily_summary
     WHERE date = '2026-08-31'",
  )
  .fetch_all(&pool)
  .await
  .unwrap();
  pool.close().await;
  assert_eq!(
    stored,
    vec!["null".to_owned()],
    "SQLite stores a bound NaN as NULL, which is the behaviour being matched"
  );

  let mut owned = Owned::open("nan.duckdb").await;
  native_daily::upsert(
    owned.database(),
    cancel(),
    DailyCoolingSummary {
      date: day("2026-09-07"),
      coverage_minutes: 0,
      idle: nan_band(),
      low: nan_band(),
      mid: nan_band(),
      high: nan_band(),
      power: PowerSummary {
        avg: Some(f32::NAN),
        max: Some(f32::NAN),
        min: Some(f32::NAN),
        sample_minutes: 0,
      },
    },
  )
  .await
  .unwrap();
  native_hourly::upsert(
    owned.database(),
    cancel(),
    HourlyCoolingSummary {
      hour_start: hour("2026-09-07 00:00:00"),
      cpu_usage_avg: Some(f32::NAN),
      cpu_temperature_avg: Some(f32::NAN),
      sample_minutes: 0,
    },
  )
  .await
  .unwrap();

  // Read back through the reader the application uses: a NaN that survived the
  // bind would come back as `Some(NaN)`, and `Some(NaN) != None`.
  let written =
    native_daily::select_all_daily_cooling_summaries(owned.database(), cancel())
      .await
      .unwrap()
      .into_iter()
      .find(|summary| summary.date == day("2026-09-07"))
      .unwrap();
  assert_eq!(written.idle, empty_band());
  assert_eq!(written.power.avg, None);
  assert_eq!(
    native_hourly::select_hours_in_date_range(
      owned.database(),
      cancel(),
      day("2026-09-07"),
      day("2026-09-07")
    )
    .await
    .unwrap()[0]
      .cpu_usage_avg,
    None
  );
  owned.close().await;

  // And the column really holds NULL rather than a DuckDB NaN. The typed
  // reader above cannot tell those apart on its own, because narrowing a
  // `f64::NAN` to `f32` is still NaN - so the storage itself is checked.
  let connection = native_support::read_only(&owned.path);
  let kind: String = connection
    .query_row(
      "SELECT CASE WHEN idle_cpu_temperature_avg IS NULL THEN 'null'
                   WHEN isnan(idle_cpu_temperature_avg) THEN 'nan'
                   ELSE 'real' END
       FROM cooling_daily_summary WHERE date = '2026-09-07'",
      [],
      |row| row.get(0),
    )
    .unwrap();
  assert_eq!(kind, "null");
}

/// A `NOT NULL` real column is the case where SQLite does not quietly absorb
/// the NaN: the insert fails outright. The native writer must fail too, and
/// must say which column carried it rather than surfacing a DuckDB constraint
/// message - or, worse, storing a NaN neither engine would have.
#[tokio::test]
async fn a_nan_in_a_required_column_is_refused_natively() {
  let mut owned = Owned::open("nan-required.duckdb").await;

  // Through the rollup, so the refusal is shown where it actually happens -
  // and the day it was part of rolls back with it.
  let error = native_rollup::persist_day_rollup(
    owned.database(),
    cancel(),
    DayRollup {
      summary: Some(DailyCoolingSummary {
        date: day("2026-09-07"),
        coverage_minutes: 10,
        idle: band(40.0, 10),
        low: empty_band(),
        mid: empty_band(),
        high: empty_band(),
        power: PowerSummary {
          avg: None,
          max: None,
          min: None,
          sample_minutes: 0,
        },
      }),
      fans: vec![FanDailySummary {
        date: day("2026-09-07"),
        source: "Exhaust".to_owned(),
        rpm_avg: f32::NAN,
        rpm_max: 1800,
        rpm_min: 700,
        sample_minutes: 10,
      }],
      ..DayRollup::default()
    },
  )
  .await
  .unwrap_err();
  assert!(
    matches!(
      error,
      NativeDatabaseError::NotANumberInRequiredColumn {
        table: "cooling_fan_daily_summary",
        column: "rpm_avg"
      }
    ),
    "{error:?}"
  );

  // The refused fan row left nothing behind - and neither did the daily row
  // already written in the same transaction.
  assert!(
    !native_fan_daily::select_all_fan_daily_summaries(owned.database(), cancel())
      .await
      .unwrap()
      .iter()
      .any(|fan| fan.date == day("2026-09-07"))
  );
  assert!(
    !native_daily::select_all_daily_cooling_summaries(owned.database(), cancel())
      .await
      .unwrap()
      .iter()
      .any(|summary| summary.date == day("2026-09-07"))
  );

  let error = native_baseline::insert_established_baseline(
    owned.database(),
    cancel(),
    &EstablishedBaseline {
      idle_temperature_avg: f32::NAN,
      window_start_date: day("2026-01-01"),
      window_end_date: day("2026-01-07"),
      sample_minutes: 1,
    },
    at("2027-01-01T00:00:00Z"),
  )
  .await
  .unwrap_err();
  assert!(
    matches!(
      error,
      NativeDatabaseError::NotANumberInRequiredColumn {
        table: "cooling_baseline",
        column: "idle_temperature_avg"
      }
    ),
    "{error:?}"
  );
  owned.close().await;
}
