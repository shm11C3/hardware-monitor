//! Hardware-archive worker.
//!
//! The worker subscribes to [`crate::event_bus::EventBus`] and maintains
//! its own rolling buffers of per-second samples. Every
//! [`HARDWARE_ARCHIVE_INTERVAL_SECONDS`] it computes avg/min/max stats
//! and writes them through [`crate::infrastructure::database`].
//!
//! Persistence intentionally does **not** share the collector's
//! `HistoryStore` `Arc<Mutex<...>>` bag: the only path from collector to
//! persistence is the broadcast channel, which means a slow DB write
//! cannot back-pressure sensor polling.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use tokio::runtime::Handle;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{Duration, MissedTickBehavior, interval};

use crate::event_bus::EventBus;
use crate::infrastructure::providers::environmental::EnvironmentalSensorRegistry;
use crate::models::{
  FanSpeedStatus, MetricsSnapshot, MotherboardFanSpeed, ProcessSample,
};
use crate::persistence::archive_data::{
  AmbientData, FanArchiveRow, GpuData, HardwareArchiveRow, HardwareData, ProcessStatData,
};
use crate::{log_info, log_warn};

/// Interval between archive writes. Matches the 60-sample history
/// buffer the collector keeps, so each archive row summarizes one
/// minute of samples.
pub const HARDWARE_ARCHIVE_INTERVAL_SECONDS: u64 = 60;

const PROCESS_RECORD_LIMIT: usize = 5;
const PROCESS_HISTORY_BUFFER: usize = 60;

#[derive(Debug, Clone, Copy)]
enum ProcessRankingMetric {
  Cpu,
  Memory,
  ExecutionTime,
}

impl ProcessRankingMetric {
  const ALL: [Self; 3] = [Self::Cpu, Self::Memory, Self::ExecutionTime];
}

/// Background controller for the archive worker. Returned by
/// [`ArchiveController::setup`]; call [`ArchiveController::terminate`]
/// to drain the task on shutdown.
pub struct ArchiveController {
  handle: JoinHandle<()>,
  stop_tx: watch::Sender<bool>,
}

impl ArchiveController {
  /// Spawn the archive worker on `runtime`. The worker subscribes to
  /// `bus` immediately so no snapshots are missed between setup and the
  /// first tick.
  pub fn setup(bus: &EventBus, runtime: Handle) -> Self {
    Self::setup_with_environmental_sensors(bus, runtime, Arc::default())
  }

  /// [`ArchiveController::setup`] with ambient sources attached (#2043).
  ///
  /// The registry rides the same one-minute tick as the hardware row, so
  /// an ambient reading and the CPU temperature it explains land on the
  /// same minute. An empty registry behaves exactly like [`Self::setup`].
  pub fn setup_with_environmental_sensors(
    bus: &EventBus,
    runtime: Handle,
    environmental_sensors: Arc<EnvironmentalSensorRegistry>,
  ) -> Self {
    let (stop_tx, mut stop_rx) = watch::channel(false);
    let mut rx = bus.subscribe();

    let handle = runtime.spawn(async move {
      let mut tracker = ArchiveTracker::with_environmental_sensors(environmental_sensors);
      let mut ticker =
        interval(Duration::from_secs(HARDWARE_ARCHIVE_INTERVAL_SECONDS));
      ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
      // The first tick fires immediately; consume it so the first archive
      // write is delayed by a full interval and has data to summarize.
      ticker.tick().await;

      loop {
        // `biased;` makes `tokio::select!` poll branches top-to-bottom
        // deterministically. Order matters: shutdown stays at the top
        // so a stop signal is never delayed; the periodic archive
        // ticker comes next so a continuously-ready snapshot stream
        // (e.g. when the collector temporarily out-paces us, or after
        // a `Lagged` recovery) cannot starve the 60-second write;
        // `rx.recv()` is last and may shed a snapshot in favor of an
        // overdue write - which is fine because the tracker only ever
        // computes aggregates over its 60-sample rings.
        tokio::select! {
          biased;
          changed = stop_rx.changed() => {
            if changed.is_err() || *stop_rx.borrow() {
              log_info!(
                "archive worker shutdown signal received",
                "persistence::archive",
                None::<&str>
              );
              break;
            }
          }
          _ = ticker.tick() => {
            let start = std::time::Instant::now();
            tracker.write_archive().await;
            let elapsed = start.elapsed();
            if elapsed > Duration::from_secs(HARDWARE_ARCHIVE_INTERVAL_SECONDS) {
              log_warn!(
                &format!("overrun {:?} (> {}s)", elapsed, HARDWARE_ARCHIVE_INTERVAL_SECONDS),
                "persistence::archive",
                None::<&str>
              );
            }
          }
          received = rx.recv() => match received {
            Ok(snapshot) => tracker.ingest(snapshot),
            Err(RecvError::Lagged(skipped)) => {
              log_warn!(
                &format!("archive worker lagged, dropped {skipped} snapshot(s)"),
                "persistence::archive",
                None::<&str>
              );
            }
            Err(RecvError::Closed) => break,
          },
        }
      }

      // Phase 5 (#1408): write a final summary on shutdown so samples
      // accumulated since the last 60s tick are not lost when the user
      // explicitly quits. `is_dirty()` skips the flush when nothing has
      // been ingested since the previous write - common when shutdown
      // lands within a fraction of a second after a scheduled tick.
      if tracker.is_dirty() {
        tracker.write_archive().await;
      }
    });

    Self { handle, stop_tx }
  }

  pub async fn terminate(self) {
    let _ = self.stop_tx.send(true);
    let _ = self.handle.await;
  }
}

/// Delete archive rows older than `retention_days`. Errors are logged
/// per-table so a partial failure (e.g. a table missing on an old
/// schema) doesn't suppress cleanup of the remaining tables.
///
/// This is a **one-shot** operation: it deletes everything older than
/// the cutoff at the moment of the call and then returns. App calls it
/// once at startup when `hardware_archive.scheduled_data_deletion` is
/// enabled - that matches the pre-Phase-4 behavior of the previous
/// `batch_delete_old_data` wrapper. The cleanup trigger is still
/// "next process boot", not a recurring schedule.
///
/// Also runs the cooling daily rollup's own retention cleanup at this
/// same `scheduled_data_deletion`-gated startup site. It intentionally
/// does **not** use `retention_days` (that is `hardwareArchive.retentionDays`,
/// user-configurable and defaulting to 30 days): the whole point of the
/// daily rollup is to outlive the one-minute archive rows it is derived
/// from, so it keeps its own fixed, independent retention window - see
/// `crate::persistence::cooling_rollup::COOLING_DAILY_SUMMARY_RETENTION_DAYS`.
pub async fn cleanup_old_data(retention_days: u32) {
  use crate::infrastructure::database;
  use crate::log_error;

  if let Err(e) = database::hardware_archive::delete_old_data(retention_days).await {
    log_error!(
      "Failed to delete old hardware archive data",
      "persistence::archive::cleanup_old_data",
      Some(e.to_string())
    );
  }

  if let Err(e) = database::gpu_archive::delete_old_data(retention_days).await {
    log_error!(
      "Failed to delete old GPU hardware archive data",
      "persistence::archive::cleanup_old_data",
      Some(e.to_string())
    );
  }

  if let Err(e) = database::fan_archive::delete_old_data(retention_days).await {
    log_error!(
      "Failed to delete old fan archive data",
      "persistence::archive::cleanup_old_data",
      Some(e.to_string())
    );
  }

  if let Err(e) = database::process_stats::delete_old_data(retention_days).await {
    log_error!(
      "Failed to delete old process stats data",
      "persistence::archive::cleanup_old_data",
      Some(e.to_string())
    );
  }

  if let Err(e) = database::ambient_archive::delete_old_data(retention_days).await {
    log_error!(
      "Failed to delete old ambient archive data",
      "persistence::archive::cleanup_old_data",
      Some(e.to_string())
    );
  }

  crate::persistence::cooling_rollup::cleanup_old_data().await;
}

// ── Internal: per-snapshot accumulator ────────────────────────────────

/// How long a vanished process is kept around before it's evicted from
/// [`ArchiveTracker::processes`]. Sized to the archive interval so a
/// short-lived process that exits within the active window still
/// appears in the next archive row, then drops on the following tick.
const PROCESS_GRACE_TICKS: u64 = HARDWARE_ARCHIVE_INTERVAL_SECONDS;

/// Cached process info accumulated from EventBus payloads. `name` and
/// `run_time_secs` come from the most recent snapshot; the CPU and
/// memory rings are 60-sample windows used to compute averages on the
/// archive tick. `last_seen_tick` lets the tracker keep a process
/// around for one full archive window after it disappears so a
/// short-lived offender that exits late in the window is still archived.
#[derive(Default)]
struct ProcessAccumulator {
  name: String,
  run_time_secs: u64,
  cpu_history: VecDeque<f32>,
  memory_kb_history: VecDeque<f32>,
  last_seen_tick: u64,
}

#[derive(Default)]
struct ArchiveTracker {
  cpu_history: VecDeque<f32>,
  cpu_temperature_history: VecDeque<Option<f32>>,
  cpu_power_history: VecDeque<Option<f32>>,
  gpu_power_history: VecDeque<Option<f32>>,
  ane_power_history: VecDeque<Option<f32>>,
  package_power_history: VecDeque<Option<f32>>,
  memory_history: VecDeque<f32>,
  gpu_usage_histories: HashMap<String, VecDeque<f32>>,
  gpu_temperature_histories: HashMap<String, VecDeque<i32>>,
  gpu_dedicated_memory_histories: HashMap<String, VecDeque<i32>>,
  gpu_name_map: HashMap<String, String>,
  /// One rolling window of per-second fan readings per fan, keyed by the
  /// fan's stable channel-derived name (#2022). `None` marks a second the
  /// fan reported nothing archivable, which is what lets a fan that stops
  /// being reported age out of its own window instead of having its last
  /// reading re-archived every minute.
  fan_rpm_histories: HashMap<String, VecDeque<Option<u32>>>,
  processes: HashMap<u32, ProcessAccumulator>,
  /// Monotonic counter incremented on every `ingest` call. Used to age
  /// out vanished processes via [`PROCESS_GRACE_TICKS`].
  tick_counter: u64,
  /// Number of CPU cores observed in the most recent snapshot. Used to
  /// normalize sysinfo's per-core CPU values to a 0-100 percentage.
  cores: f32,
  /// Set on every `ingest`; cleared by `write_archive`. Drives the
  /// shutdown-time final flush so we skip the write when nothing has
  /// been ingested since the previous tick (e.g. shutdown lands within
  /// a fraction of a second after a scheduled write).
  dirty: bool,
  /// Ambient sources polled on the archive tick. Empty on installs with
  /// no environmental sensor, which then write no ambient rows at all.
  environmental_sensors: Arc<EnvironmentalSensorRegistry>,
}

impl ArchiveTracker {
  fn new() -> Self {
    Self::default()
  }

  fn with_environmental_sensors(
    environmental_sensors: Arc<EnvironmentalSensorRegistry>,
  ) -> Self {
    Self {
      environmental_sensors,
      ..Self::new()
    }
  }

  fn is_dirty(&self) -> bool {
    self.dirty
  }

  fn ingest(&mut self, snapshot: MetricsSnapshot) {
    self.dirty = true;
    self.tick_counter = self.tick_counter.saturating_add(1);
    push_capped(&mut self.cpu_history, snapshot.cpu_usage);
    push_capped_optional(&mut self.cpu_temperature_history, snapshot.cpu_temperature);
    push_capped_optional(&mut self.cpu_power_history, snapshot.power_draw.cpu_watts);
    push_capped_optional(&mut self.gpu_power_history, snapshot.power_draw.gpu_watts);
    push_capped_optional(&mut self.ane_power_history, snapshot.power_draw.ane_watts);
    push_capped_optional(
      &mut self.package_power_history,
      snapshot.power_draw.package_watts,
    );
    push_capped(&mut self.memory_history, snapshot.memory_usage);

    if !snapshot.processors_usage.is_empty() {
      self.cores = snapshot.processors_usage.len() as f32;
    }

    for gpu in snapshot.gpus {
      self.gpu_name_map.insert(gpu.gpu_id.clone(), gpu.gpu_name);
      if let Some(usage) = gpu.gpu_usage {
        push_capped(
          self
            .gpu_usage_histories
            .entry(gpu.gpu_id.clone())
            .or_default(),
          usage,
        );
      }
      if let Some(temperature) = gpu.gpu_temperature {
        push_capped_i32(
          self
            .gpu_temperature_histories
            .entry(gpu.gpu_id.clone())
            .or_default(),
          temperature as i32,
        );
      }
      if let Some(memory_kb) = gpu.gpu_dedicated_memory_usage_kb {
        push_capped_i32(
          self
            .gpu_dedicated_memory_histories
            .entry(gpu.gpu_id.clone())
            .or_default(),
          memory_kb as i32,
        );
      }
    }

    self.ingest_fan_speeds(snapshot.motherboard_fan_speeds);
    self.update_process_accumulators(snapshot.processes);
  }

  /// Fold one snapshot's live fan readings into the per-fan rolling
  /// windows, preserving the three distinct fan-reading meanings (#2022):
  /// an Inactive Fan Reading (0 RPM) is stored as the real observation it
  /// is, an Invalid Fan Reading contributes nothing, and a missing reading
  /// stays absent. None of them collapse into each other.
  fn ingest_fan_speeds(&mut self, fan_speeds: Vec<MotherboardFanSpeed>) {
    let mut reported: HashSet<String> = HashSet::with_capacity(fan_speeds.len());

    for fan in fan_speeds {
      let rpm = archivable_fan_rpm(&fan);
      reported.insert(fan.name.clone());
      push_capped_optional_u32(self.fan_rpm_histories.entry(fan.name).or_default(), rpm);
    }

    // A fan the provider stopped reporting still ages through its own
    // window, so its last reading cannot be re-archived minute after
    // minute once the sensor is gone.
    for (name, history) in self.fan_rpm_histories.iter_mut() {
      if !reported.contains(name) {
        push_capped_optional_u32(history, None);
      }
    }

    // Once a window holds nothing but absences the fan has contributed
    // nothing for a full interval; drop it so the map cannot grow without
    // bound across provider or hardware changes.
    self
      .fan_rpm_histories
      .retain(|_, history| history.iter().any(Option::is_some));
  }

  fn update_process_accumulators(&mut self, samples: Vec<ProcessSample>) {
    let now = self.tick_counter;

    // Age out accumulators that haven't been refreshed within the
    // grace window. Evicting on every snapshot (the previous behavior)
    // dropped processes that exited just before the archive tick - a
    // short-lived offender that burned CPU for most of the minute and
    // then exited would never appear in the archive row. Keeping
    // entries for [`PROCESS_GRACE_TICKS`] solves that without letting
    // the map grow unbounded.
    self
      .processes
      .retain(|_pid, acc| now.saturating_sub(acc.last_seen_tick) <= PROCESS_GRACE_TICKS);

    for sample in samples {
      let entry = self.processes.entry(sample.pid).or_default();
      // Detect PID reuse via name change rather than absence length.
      // Brief gaps (broadcast lag, a transient sysinfo miss) shouldn't
      // throw away rolling history, but a recycled PID running a
      // *different* binary should not inherit the prior process's
      // samples. Comparing against the cached name catches the latter
      // without penalising the former.
      //
      // Trade-off: a recycled PID running the same binary name is
      // indistinguishable from the original here and will continue
      // accumulating into the existing rings. That's a rare and
      // benign case - the values stay representative of a process
      // with that name on that PID slot.
      let pid_reused =
        entry.last_seen_tick != 0 && !entry.name.is_empty() && entry.name != sample.name;
      if pid_reused {
        entry.cpu_history.clear();
        entry.memory_kb_history.clear();
      }
      entry.name = sample.name;
      entry.run_time_secs = sample.run_time_secs;
      entry.last_seen_tick = now;
      push_capped(&mut entry.cpu_history, sample.cpu_usage);
      push_capped(&mut entry.memory_kb_history, sample.memory_kb);
    }
  }

  async fn write_archive(&mut self) {
    use crate::infrastructure::database;
    use crate::log_error;

    self.dirty = false;

    // Taken once at the top of the write cycle and threaded into every
    // insert below, so a cycle that straddles a minute boundary cannot
    // scatter its rows across two minutes. Two things break when it does.
    //
    // Measurements folded from the same snapshots land in adjacent
    // buckets, breaking the alignment the synchronized timeline lanes
    // depend on (#2050). And the ambient pairing join (#2045) is defined
    // on exactly that minute, so a hardware row and the ambient row
    // explaining it stop pairing at all - selectively, and in the worst
    // possible places: a slow cycle is a busy one, so the minutes most
    // worth explaining were the likeliest to lose their ambient reading.
    let tick_timestamp = chrono::Utc::now();

    let cpu = StatsCalculator::compute_stats(self.cpu_history.iter().copied());
    let cpu_temperature = StatsCalculator::compute_stats(
      self.cpu_temperature_history.iter().copied().flatten(),
    );
    let memory = StatsCalculator::compute_stats(self.memory_history.iter().copied());
    let cpu_power =
      StatsCalculator::compute_stats(self.cpu_power_history.iter().copied().flatten());
    let gpu_power =
      StatsCalculator::compute_stats(self.gpu_power_history.iter().copied().flatten());
    let ane_power =
      StatsCalculator::compute_stats(self.ane_power_history.iter().copied().flatten());
    let package_power = StatsCalculator::compute_stats(
      self.package_power_history.iter().copied().flatten(),
    );

    let row = HardwareArchiveRow {
      cpu,
      memory,
      cpu_temperature,
      cpu_power,
      gpu_power,
      ane_power,
      package_power,
    };
    if let Err(e) = database::hardware_archive::insert(row, tick_timestamp).await {
      log_error!(
        "Failed to insert hardware archive data",
        "persistence::archive::write_archive",
        Some(e.to_string())
      );
    }

    for gpu in self.collect_gpu_data() {
      if let Err(e) = database::gpu_archive::insert(gpu, tick_timestamp).await {
        log_error!(
          "Failed to insert GPU hardware archive data",
          "persistence::archive::write_archive",
          Some(e.to_string())
        );
      }
    }

    // Written on the same tick as the row above so a fan reading and the
    // temperature it belongs with share one minute boundary.
    let fans = self.collect_fan_data();
    if !fans.is_empty()
      && let Err(e) = database::fan_archive::insert(fans, tick_timestamp).await
    {
      log_error!(
        "Failed to insert fan archive data",
        "persistence::archive::write_archive",
        Some(e.to_string())
      );
    }

    let process_stats = self.collect_process_stats();
    if !process_stats.is_empty()
      && let Err(e) = database::process_stats::insert(process_stats, tick_timestamp).await
    {
      log_error!(
        "Failed to insert process stats data",
        "persistence::archive::write_archive",
        Some(e.to_string())
      );
    }

    // Ambient rows are keyed to the tick, not to each sensor's own
    // observation time, so they join the hardware row for this minute.
    let ambient = self.collect_ambient_data(tick_timestamp);
    if !ambient.is_empty()
      && let Err(e) = database::ambient_archive::insert(ambient, tick_timestamp).await
    {
      log_error!(
        "Failed to insert ambient archive data",
        "persistence::archive::write_archive",
        Some(e.to_string())
      );
    }
  }

  /// The ambient rows for the minute ending at `now` - one per source
  /// with a fresh reading, and none for a source that has gone quiet.
  fn collect_ambient_data(&self, now: chrono::DateTime<chrono::Utc>) -> Vec<AmbientData> {
    self
      .environmental_sensors
      .fresh_readings(now)
      .into_iter()
      .map(|reading| AmbientData {
        source: reading.source,
        temperature: reading.temperature_celsius,
        humidity: reading.humidity_percent,
      })
      .collect()
  }

  fn collect_gpu_data(&self) -> Vec<GpuData> {
    // Walk the union of every GPU history map so adapters that report
    // only temperature or only dedicated memory (e.g. Linux DRM, ADL
    // without VRAM, Apple IOKit without thermals) still produce a row
    // - iterating `gpu_usage_histories` alone would silently drop them.
    let gpu_ids: HashSet<&String> = self
      .gpu_usage_histories
      .keys()
      .chain(self.gpu_temperature_histories.keys())
      .chain(self.gpu_dedicated_memory_histories.keys())
      .collect();

    // The archive API selects GPU data by display name. Keep one row per
    // name and interval so distinct live ids cannot interleave into one chart
    // when two adapters share the same name. Group the raw histories before
    // calculating aggregates so an adapter with fewer samples does not have
    // the same weight as an adapter with a full history.
    let mut ids_by_name: HashMap<String, Vec<&String>> = HashMap::new();
    for gpu_id in gpu_ids {
      let gpu_name = self
        .gpu_name_map
        .get(gpu_id)
        .cloned()
        .unwrap_or_else(|| gpu_id.clone());
      ids_by_name.entry(gpu_name).or_default().push(gpu_id);
    }

    ids_by_name
      .into_iter()
      .map(|(gpu_name, ids)| {
        let (usage_avg, usage_max, usage_min) = StatsCalculator::compute_f32_aggregates(
          ids
            .iter()
            .filter_map(|gpu_id| self.gpu_usage_histories.get(*gpu_id))
            .flat_map(|history| history.iter().copied()),
        );
        let (temp_avg, temp_max, temp_min) = StatsCalculator::compute_i32_aggregates(
          ids
            .iter()
            .filter_map(|gpu_id| self.gpu_temperature_histories.get(*gpu_id))
            .flat_map(|history| history.iter().copied()),
        );
        let (mem_avg_f32, mem_max, mem_min) = StatsCalculator::compute_i32_aggregates(
          ids
            .iter()
            .filter_map(|gpu_id| self.gpu_dedicated_memory_histories.get(*gpu_id))
            .flat_map(|history| history.iter().copied()),
        );
        GpuData {
          gpu_id: (ids.len() == 1).then(|| ids[0].clone()),
          gpu_name,
          usage_avg,
          usage_max,
          usage_min,
          temperature_avg: temp_avg,
          temperature_max: temp_max,
          temperature_min: temp_min,
          dedicated_memory_avg: mem_avg_f32.map(|v| v as i32),
          dedicated_memory_max: mem_max,
          dedicated_memory_min: mem_min,
        }
      })
      .collect()
  }

  /// One row per fan that contributed at least one archivable reading this
  /// interval. A fan whose window holds only absences produces no row at
  /// all rather than a zero-filled one (DP-02).
  fn collect_fan_data(&self) -> Vec<FanArchiveRow> {
    self
      .fan_rpm_histories
      .iter()
      .filter_map(|(source, history)| {
        let readings: Vec<u32> = history.iter().copied().flatten().collect();
        if readings.is_empty() {
          return None;
        }
        // Sum in u64: a full window of readings cannot overflow, but the
        // accumulator should not depend on that being audited again.
        let sum: u64 = readings.iter().map(|&rpm| rpm as u64).sum();
        Some(FanArchiveRow {
          source: source.clone(),
          rpm: (sum / readings.len() as u64) as u32,
        })
      })
      .collect()
  }

  fn collect_process_stats(&self) -> Vec<ProcessStatData> {
    let core_divisor = self.cores.max(1.0);
    let all_stats: Vec<ProcessStatData> = self
      .processes
      .iter()
      .filter_map(|(pid, acc)| {
        if acc.cpu_history.is_empty() || acc.memory_kb_history.is_empty() {
          return None;
        }
        let cpu_avg = acc.cpu_history.iter().sum::<f32>() / acc.cpu_history.len() as f32;
        let mem_avg =
          acc.memory_kb_history.iter().sum::<f32>() / acc.memory_kb_history.len() as f32;
        if cpu_avg == 0.0 && mem_avg == 0.0 {
          return None;
        }
        let exec_time = acc.run_time_secs as i32;
        if !is_valid_execution_time(exec_time) {
          return None;
        }
        Some(ProcessStatData {
          pid: *pid as i32,
          process_name: acc.name.clone(),
          cpu_usage: cpu_avg / core_divisor,
          memory_usage: mem_avg.round() as i32,
          execution_sec: exec_time,
        })
      })
      .collect();

    rank_top_processes(all_stats)
  }
}

/// The RPM one live fan reading contributes to the archive, or `None` when
/// it contributes nothing (#2022).
///
/// [`FanSpeedStatus::Invalid`] is excluded because the reported value is
/// outside the accepted reading shape - archiving it would persist a number
/// the live display already refuses to show. A `None` `rpm` is simply
/// absent. Everything else, including an Inactive Fan Reading of 0 RPM, is
/// a real observation and archived as such.
fn archivable_fan_rpm(fan: &MotherboardFanSpeed) -> Option<u32> {
  match fan.status {
    FanSpeedStatus::Invalid => None,
    FanSpeedStatus::Active | FanSpeedStatus::Inactive => fan.rpm,
  }
}

fn is_valid_execution_time(exec_time: i32) -> bool {
  (0..=60 * 60 * 24 * 30).contains(&exec_time)
}

fn rank_top_processes(all_stats: Vec<ProcessStatData>) -> Vec<ProcessStatData> {
  let mut result = Vec::new();
  let mut seen = HashSet::new();
  for &metric in &ProcessRankingMetric::ALL {
    let sorted = sort_by_metric(all_stats.clone(), metric);
    for stat in sorted.into_iter().take(PROCESS_RECORD_LIMIT) {
      if seen.insert(stat.pid) {
        result.push(stat);
      }
    }
    if result.len() >= PROCESS_RECORD_LIMIT * ProcessRankingMetric::ALL.len() {
      break;
    }
  }
  result
}

fn sort_by_metric(
  mut stats: Vec<ProcessStatData>,
  metric: ProcessRankingMetric,
) -> Vec<ProcessStatData> {
  match metric {
    ProcessRankingMetric::Cpu => {
      stats.sort_by(|a, b| b.cpu_usage.total_cmp(&a.cpu_usage))
    }
    ProcessRankingMetric::Memory => {
      stats.sort_by_key(|s| std::cmp::Reverse(s.memory_usage))
    }
    ProcessRankingMetric::ExecutionTime => {
      stats.sort_by_key(|s| std::cmp::Reverse(s.execution_sec))
    }
  }
  stats
}

fn push_capped(buf: &mut VecDeque<f32>, value: f32) {
  if buf.len() >= PROCESS_HISTORY_BUFFER {
    buf.pop_front();
  }
  buf.push_back(value);
}

fn push_capped_optional(buf: &mut VecDeque<Option<f32>>, value: Option<f32>) {
  if buf.len() >= PROCESS_HISTORY_BUFFER {
    buf.pop_front();
  }
  buf.push_back(value);
}

fn push_capped_optional_u32(buf: &mut VecDeque<Option<u32>>, value: Option<u32>) {
  if buf.len() >= PROCESS_HISTORY_BUFFER {
    buf.pop_front();
  }
  buf.push_back(value);
}

fn push_capped_i32(buf: &mut VecDeque<i32>, value: i32) {
  if buf.len() >= PROCESS_HISTORY_BUFFER {
    buf.pop_front();
  }
  buf.push_back(value);
}

// ── Stats helpers ─────────────────────────────────────────────────────

struct StatsCalculator;

impl StatsCalculator {
  fn compute_stats(values: impl IntoIterator<Item = f32>) -> HardwareData {
    let values: Vec<f32> = values.into_iter().collect();
    if values.is_empty() {
      return HardwareData {
        avg: None,
        max: None,
        min: None,
      };
    }
    let avg = Some(values.iter().sum::<f32>() / values.len() as f32);
    let max = values.iter().copied().max_by(f32::total_cmp);
    let min = values.iter().copied().min_by(f32::total_cmp);
    HardwareData { avg, max, min }
  }

  fn compute_f32_aggregates(
    values: impl IntoIterator<Item = f32>,
  ) -> (Option<f32>, Option<f32>, Option<f32>) {
    let values: Vec<f32> = values.into_iter().collect();
    if values.is_empty() {
      return (None, None, None);
    }
    let avg = Some(values.iter().sum::<f32>() / values.len() as f32);
    let max = values.iter().copied().max_by(f32::total_cmp);
    let min = values.iter().copied().min_by(f32::total_cmp);
    (avg, max, min)
  }

  fn compute_i32_aggregates(
    values: impl IntoIterator<Item = i32>,
  ) -> (Option<f32>, Option<i32>, Option<i32>) {
    let values: Vec<i32> = values.into_iter().collect();
    if values.is_empty() {
      return (None, None, None);
    }
    // Sum in i64: dedicated GPU memory is reported in KB, so 60 samples
    // from a high-VRAM card (e.g. 80 GB is about 84,000,000 KB) easily exceed
    // i32::MAX. An i32 accumulator panics in debug and silently wraps
    // in release.
    let sum: i64 = values.iter().map(|&v| v as i64).sum();
    let avg = Some(sum as f32 / values.len() as f32);
    let max = values.iter().copied().max();
    let min = values.iter().copied().min();
    (avg, max, min)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::models::GpuMetric;

  fn make_snapshot(cpu: f32, memory: f32, processors: usize) -> MetricsSnapshot {
    MetricsSnapshot {
      cpu_usage: cpu,
      memory_usage: memory,
      processors_usage: vec![0.0; processors],
      gpus: vec![],
      processes: vec![],
      cpu_temperature: None,
      cpu_package_thermal_status: None,
      sensor_temperatures: vec![],
      motherboard_temperatures: vec![],
      motherboard_fan_speeds: vec![],
      power_draw: Default::default(),
      cpu_power_support: Default::default(),
      motherboard_fan_support: Default::default(),
      external_component_guidance_candidates: vec![],
    }
  }

  fn make_process_stat(pid: i32, cpu: f32, mem: i32, exec: i32) -> ProcessStatData {
    ProcessStatData {
      pid,
      process_name: format!("p{pid}"),
      cpu_usage: cpu,
      memory_usage: mem,
      execution_sec: exec,
    }
  }

  // ── StatsCalculator ──

  #[test]
  fn compute_stats_empty_slice_returns_all_none() {
    let result = StatsCalculator::compute_stats(std::iter::empty::<f32>());
    assert!(result.avg.is_none());
    assert!(result.max.is_none());
    assert!(result.min.is_none());
  }

  #[test]
  fn compute_stats_typical() {
    let result = StatsCalculator::compute_stats([10.0_f32, 20.0, 30.0]);
    assert_eq!(result.avg, Some(20.0));
    assert_eq!(result.max, Some(30.0));
    assert_eq!(result.min, Some(10.0));
  }

  #[test]
  fn compute_f32_aggregates_typical() {
    assert_eq!(
      StatsCalculator::compute_f32_aggregates([1.0_f32, 2.0, 3.0, 4.0, 5.0]),
      (Some(3.0), Some(5.0), Some(1.0))
    );
  }

  #[test]
  fn compute_i32_aggregates_typical() {
    assert_eq!(
      StatsCalculator::compute_i32_aggregates([10, 20, 30]),
      (Some(20.0), Some(30), Some(10))
    );
  }

  // ── fan speed archiving (#2022) ──

  fn fan(name: &str, rpm: Option<u32>, status: FanSpeedStatus) -> MotherboardFanSpeed {
    MotherboardFanSpeed {
      name: name.to_string(),
      rpm,
      status,
      source: "NCT6799D / Super I/O".to_string(),
    }
  }

  fn fan_rows(tracker: &ArchiveTracker) -> Vec<FanArchiveRow> {
    let mut rows = tracker.collect_fan_data();
    rows.sort_by(|a, b| a.source.cmp(&b.source));
    rows
  }

  #[test]
  fn an_active_fan_reading_is_archived_per_fan() {
    let mut tracker = ArchiveTracker::new();
    let mut snapshot = make_snapshot(10.0, 50.0, 4);
    snapshot.motherboard_fan_speeds = vec![
      fan("Fan 1", Some(900), FanSpeedStatus::Active),
      fan("Fan 2", Some(1500), FanSpeedStatus::Active),
    ];
    tracker.ingest(snapshot);

    assert_eq!(
      fan_rows(&tracker),
      vec![
        FanArchiveRow {
          source: "Fan 1".to_string(),
          rpm: 900,
        },
        FanArchiveRow {
          source: "Fan 2".to_string(),
          rpm: 1500,
        },
      ]
    );
  }

  #[test]
  fn an_inactive_fan_reading_is_archived_as_a_real_zero() {
    // 0 RPM is an Inactive Fan Reading - a real observation that the fan is
    // not reporting rotation, not a missing one. Dropping it would make a
    // stopped fan indistinguishable from an unreadable one.
    let mut tracker = ArchiveTracker::new();
    let mut snapshot = make_snapshot(10.0, 50.0, 4);
    snapshot.motherboard_fan_speeds =
      vec![fan("Fan 3", Some(0), FanSpeedStatus::Inactive)];
    tracker.ingest(snapshot);

    assert_eq!(
      fan_rows(&tracker),
      vec![FanArchiveRow {
        source: "Fan 3".to_string(),
        rpm: 0,
      }]
    );
  }

  #[test]
  fn an_invalid_fan_reading_is_excluded_rather_than_archived() {
    let mut tracker = ArchiveTracker::new();
    let mut snapshot = make_snapshot(10.0, 50.0, 4);
    snapshot.motherboard_fan_speeds =
      vec![fan("Fan 4", Some(65535), FanSpeedStatus::Invalid)];
    tracker.ingest(snapshot);

    assert_eq!(fan_rows(&tracker), Vec::new());
  }

  #[test]
  fn an_absent_fan_reading_writes_no_row() {
    let mut tracker = ArchiveTracker::new();
    let mut snapshot = make_snapshot(10.0, 50.0, 4);
    snapshot.motherboard_fan_speeds = vec![fan("Fan 5", None, FanSpeedStatus::Active)];
    tracker.ingest(snapshot);

    assert_eq!(fan_rows(&tracker), Vec::new());
  }

  #[test]
  fn a_fan_row_averages_the_minutes_archivable_readings() {
    // The excluded Invalid sample must not drag the average toward zero:
    // it contributes nothing at all, exactly like an absent reading.
    let mut tracker = ArchiveTracker::new();
    for (rpm, status) in [
      (Some(1000), FanSpeedStatus::Active),
      (Some(0), FanSpeedStatus::Inactive),
      (Some(65535), FanSpeedStatus::Invalid),
    ] {
      let mut snapshot = make_snapshot(10.0, 50.0, 4);
      snapshot.motherboard_fan_speeds = vec![fan("Fan 1", rpm, status)];
      tracker.ingest(snapshot);
    }

    assert_eq!(
      fan_rows(&tracker),
      vec![FanArchiveRow {
        source: "Fan 1".to_string(),
        rpm: 500,
      }]
    );
  }

  #[test]
  fn a_machine_with_no_fan_readings_archives_no_fan_rows() {
    let mut tracker = ArchiveTracker::new();
    tracker.ingest(make_snapshot(10.0, 50.0, 4));

    assert_eq!(fan_rows(&tracker), Vec::new());
  }

  #[test]
  fn a_fan_that_stops_reporting_expires_from_the_rolling_window() {
    let mut tracker = ArchiveTracker::new();
    let mut snapshot = make_snapshot(10.0, 50.0, 4);
    snapshot.motherboard_fan_speeds =
      vec![fan("Fan 1", Some(900), FanSpeedStatus::Active)];
    tracker.ingest(snapshot);

    for _ in 0..PROCESS_HISTORY_BUFFER {
      tracker.ingest(make_snapshot(10.0, 50.0, 4));
    }

    assert_eq!(
      fan_rows(&tracker),
      Vec::new(),
      "a fan absent for a whole window must stop producing rows, not repeat its last reading"
    );
  }

  // ── push_capped ──

  #[test]
  fn push_capped_evicts_oldest_at_buffer_limit() {
    let mut buf: VecDeque<f32> = (0..PROCESS_HISTORY_BUFFER).map(|i| i as f32).collect();
    push_capped(&mut buf, 999.0);
    assert_eq!(buf.len(), PROCESS_HISTORY_BUFFER);
    assert_eq!(*buf.front().unwrap(), 1.0);
    assert_eq!(*buf.back().unwrap(), 999.0);
  }

  // ── ArchiveTracker::ingest ──

  #[test]
  fn ingest_appends_cpu_and_memory() {
    let mut t = ArchiveTracker::new();
    t.ingest(make_snapshot(10.0, 50.0, 4));
    t.ingest(make_snapshot(20.0, 60.0, 4));
    assert_eq!(
      t.cpu_history.iter().copied().collect::<Vec<_>>(),
      vec![10.0, 20.0]
    );
    assert_eq!(
      t.memory_history.iter().copied().collect::<Vec<_>>(),
      vec![50.0, 60.0]
    );
    assert_eq!(t.cores, 4.0);
  }

  #[test]
  fn ingest_preserves_missing_cpu_temperature_samples() {
    let mut t = ArchiveTracker::new();
    let mut available = make_snapshot(10.0, 50.0, 4);
    available.cpu_temperature = Some(52.0);

    t.ingest(available);
    t.ingest(make_snapshot(20.0, 60.0, 4));

    assert_eq!(
      t.cpu_temperature_history
        .iter()
        .copied()
        .collect::<Vec<_>>(),
      vec![Some(52.0), None]
    );
    let stats =
      StatsCalculator::compute_stats(t.cpu_temperature_history.iter().copied().flatten());
    assert_eq!(stats.avg, Some(52.0));
    assert_eq!(stats.max, Some(52.0));
    assert_eq!(stats.min, Some(52.0));
  }

  #[test]
  fn ingest_expires_unavailable_cpu_temperature_from_rolling_window() {
    let mut t = ArchiveTracker::new();
    let mut available = make_snapshot(10.0, 50.0, 4);
    available.cpu_temperature = Some(52.0);
    t.ingest(available);

    for _ in 0..PROCESS_HISTORY_BUFFER {
      t.ingest(make_snapshot(10.0, 50.0, 4));
    }

    assert_eq!(t.cpu_temperature_history.len(), PROCESS_HISTORY_BUFFER);
    assert!(t.cpu_temperature_history.iter().all(Option::is_none));
  }

  #[test]
  fn ingest_preserves_missing_power_samples_as_none() {
    let mut tracker = ArchiveTracker::new();
    let mut available = make_snapshot(10.0, 50.0, 4);
    available.power_draw.cpu_watts = Some(8.0);
    available.power_draw.gpu_watts = Some(4.0);
    available.power_draw.ane_watts = Some(1.0);
    available.power_draw.package_watts = Some(13.0);

    tracker.ingest(available);
    tracker.ingest(make_snapshot(20.0, 60.0, 4));

    assert_eq!(tracker.cpu_power_history, [Some(8.0), None]);
    assert_eq!(tracker.gpu_power_history, [Some(4.0), None]);
    assert_eq!(tracker.ane_power_history, [Some(1.0), None]);
    assert_eq!(tracker.package_power_history, [Some(13.0), None]);
  }

  #[test]
  fn ingest_expires_stale_power_from_rolling_window() {
    let mut tracker = ArchiveTracker::new();
    let mut available = make_snapshot(10.0, 50.0, 4);
    available.power_draw.package_watts = Some(13.0);
    tracker.ingest(available);

    for _ in 0..PROCESS_HISTORY_BUFFER {
      tracker.ingest(make_snapshot(10.0, 50.0, 4));
    }

    assert_eq!(tracker.package_power_history.len(), PROCESS_HISTORY_BUFFER);
    assert!(tracker.package_power_history.iter().all(Option::is_none));
  }

  #[test]
  fn package_power_stats_use_per_tick_package_values() {
    let mut tracker = ArchiveTracker::new();
    for watts in [10.0, 30.0, 20.0] {
      let mut snapshot = make_snapshot(10.0, 50.0, 4);
      snapshot.power_draw.package_watts = Some(watts);
      tracker.ingest(snapshot);
    }

    let stats = StatsCalculator::compute_stats(
      tracker.package_power_history.iter().copied().flatten(),
    );
    assert_eq!(
      stats,
      HardwareData {
        avg: Some(20.0),
        max: Some(30.0),
        min: Some(10.0),
      }
    );
  }

  #[test]
  fn fresh_tracker_is_not_dirty() {
    let t = ArchiveTracker::new();
    assert!(
      !t.is_dirty(),
      "an unused tracker must not trigger a shutdown flush"
    );
  }

  #[test]
  fn ingest_marks_tracker_dirty() {
    let mut t = ArchiveTracker::new();
    t.ingest(make_snapshot(10.0, 50.0, 1));
    assert!(
      t.is_dirty(),
      "ingest must set the dirty flag so shutdown writes a final summary"
    );
  }

  #[test]
  fn ingest_caps_buffers_at_history_limit() {
    let mut t = ArchiveTracker::new();
    for i in 0..PROCESS_HISTORY_BUFFER + 5 {
      t.ingest(make_snapshot(i as f32, 0.0, 1));
    }
    assert_eq!(t.cpu_history.len(), PROCESS_HISTORY_BUFFER);
    assert_eq!(*t.cpu_history.front().unwrap(), 5.0);
  }

  #[test]
  fn ingest_records_gpu_name_and_metrics() {
    let mut t = ArchiveTracker::new();
    t.ingest(MetricsSnapshot {
      cpu_usage: 0.0,
      memory_usage: 0.0,
      processors_usage: vec![0.0],
      gpus: vec![GpuMetric {
        gpu_id: "gpu:0".into(),
        gpu_name: "RTX".into(),
        gpu_usage: Some(50.0),
        gpu_temperature: Some(70.0),
        gpu_source: "Test".into(),
        gpu_dedicated_memory_usage_kb: Some(2048.0),
        gpu_cooler_level: None,
      }],
      processes: vec![],
      cpu_temperature: None,
      cpu_package_thermal_status: None,
      sensor_temperatures: vec![],
      motherboard_temperatures: vec![],
      motherboard_fan_speeds: vec![],
      power_draw: Default::default(),
      cpu_power_support: Default::default(),
      motherboard_fan_support: Default::default(),
      external_component_guidance_candidates: vec![],
    });
    assert_eq!(t.gpu_name_map.get("gpu:0").unwrap(), "RTX");
    assert_eq!(t.gpu_usage_histories.get("gpu:0").unwrap().len(), 1);
    assert_eq!(t.gpu_temperature_histories.get("gpu:0").unwrap().len(), 1);
    assert_eq!(
      t.gpu_dedicated_memory_histories.get("gpu:0").unwrap().len(),
      1
    );
  }

  fn snap_with_pid(pid: u32, name: &str) -> MetricsSnapshot {
    MetricsSnapshot {
      cpu_usage: 0.0,
      memory_usage: 0.0,
      processors_usage: vec![0.0],
      gpus: vec![],
      processes: vec![ProcessSample {
        pid,
        name: name.into(),
        cpu_usage: 10.0,
        memory_kb: 1024.0,
        run_time_secs: 60,
      }],
      cpu_temperature: None,
      cpu_package_thermal_status: None,
      sensor_temperatures: vec![],
      motherboard_temperatures: vec![],
      motherboard_fan_speeds: vec![],
      power_draw: Default::default(),
      cpu_power_support: Default::default(),
      motherboard_fan_support: Default::default(),
      external_component_guidance_candidates: vec![],
    }
  }

  #[test]
  fn ingest_keeps_recently_vanished_processes_within_grace_window() {
    let mut t = ArchiveTracker::new();
    t.ingest(snap_with_pid(100, "short_lived"));
    // Drive a few snapshots without PID 100 (well under the grace window).
    for _ in 0..5 {
      t.ingest(snap_with_pid(200, "alive"));
    }
    assert!(
      t.processes.contains_key(&100),
      "PID 100 must persist long enough to be archived in the next tick"
    );
    assert!(t.processes.contains_key(&200));
  }

  #[test]
  fn ingest_evicts_processes_absent_past_grace_window() {
    let mut t = ArchiveTracker::new();
    t.ingest(snap_with_pid(100, "short_lived"));
    // Push past the grace window with snapshots that don't include PID 100.
    for _ in 0..PROCESS_GRACE_TICKS + 1 {
      t.ingest(snap_with_pid(200, "alive"));
    }
    assert!(!t.processes.contains_key(&100));
    assert!(t.processes.contains_key(&200));
  }

  #[test]
  fn ingest_resets_rings_on_pid_reuse_with_different_name() {
    let mut t = ArchiveTracker::new();
    t.ingest(snap_with_pid(42, "victim"));
    // Drive several ticks without PID 42 - it stays in the map (still
    // within grace) but we don't push to its rings.
    for _ in 0..3 {
      t.ingest(snap_with_pid(99, "filler"));
    }
    // PID 42 reappears running a different binary. PID reuse is
    // detected via the name change, so the prior rings must not blend
    // into the new process's samples.
    t.ingest(snap_with_pid(42, "fresh"));
    let acc = t.processes.get(&42).unwrap();
    assert_eq!(acc.cpu_history.len(), 1);
    assert_eq!(acc.memory_kb_history.len(), 1);
    assert_eq!(acc.name, "fresh");
  }

  #[test]
  fn ingest_preserves_rings_on_brief_snapshot_gap() {
    let mut t = ArchiveTracker::new();
    // Build up a few samples for PID 42.
    for _ in 0..4 {
      t.ingest(snap_with_pid(42, "victim"));
    }
    let len_before = t.processes.get(&42).unwrap().cpu_history.len();
    assert_eq!(len_before, 4);

    // Skip 3 ticks (transient sysinfo miss / broadcast lag, well under
    // the grace window). PID 42 reappears with the same name.
    for _ in 0..3 {
      t.ingest(snap_with_pid(99, "filler"));
    }
    t.ingest(snap_with_pid(42, "victim"));

    let acc = t.processes.get(&42).unwrap();
    // The brief gap must not wipe the rolling history - only the new
    // sample is appended.
    assert_eq!(acc.cpu_history.len(), len_before + 1);
    assert_eq!(acc.memory_kb_history.len(), len_before + 1);
  }

  // ── collect_process_stats / rank_top_processes ──

  #[test]
  fn rank_top_processes_dedupes_across_metrics() {
    let stats = vec![
      make_process_stat(1, 99.0, 999, 9999),
      make_process_stat(2, 1.0, 1, 1),
    ];
    let ranked = rank_top_processes(stats);
    let count_1 = ranked.iter().filter(|s| s.pid == 1).count();
    assert_eq!(count_1, 1);
  }

  #[test]
  fn rank_top_processes_caps_at_limit_times_metrics() {
    let stats: Vec<_> = (0..30)
      .map(|i| make_process_stat(i, i as f32, i * 10, i * 60))
      .collect();
    let ranked = rank_top_processes(stats);
    assert!(ranked.len() <= PROCESS_RECORD_LIMIT * ProcessRankingMetric::ALL.len());
  }

  #[test]
  fn collect_process_stats_skips_zero_only_processes() {
    let mut t = ArchiveTracker::new();
    t.ingest(MetricsSnapshot {
      cpu_usage: 0.0,
      memory_usage: 0.0,
      processors_usage: vec![0.0],
      gpus: vec![],
      processes: vec![ProcessSample {
        pid: 1,
        name: "idle".into(),
        cpu_usage: 0.0,
        memory_kb: 0.0,
        run_time_secs: 60,
      }],
      cpu_temperature: None,
      cpu_package_thermal_status: None,
      sensor_temperatures: vec![],
      motherboard_temperatures: vec![],
      motherboard_fan_speeds: vec![],
      power_draw: Default::default(),
      cpu_power_support: Default::default(),
      motherboard_fan_support: Default::default(),
      external_component_guidance_candidates: vec![],
    });
    assert!(t.collect_process_stats().is_empty());
  }

  #[test]
  fn collect_process_stats_normalizes_cpu_by_cores() {
    let mut t = ArchiveTracker::new();
    t.ingest(MetricsSnapshot {
      cpu_usage: 0.0,
      memory_usage: 0.0,
      processors_usage: vec![0.0; 8],
      gpus: vec![],
      processes: vec![ProcessSample {
        pid: 1,
        name: "p".into(),
        cpu_usage: 80.0,
        memory_kb: 1024.0,
        run_time_secs: 60,
      }],
      cpu_temperature: None,
      cpu_package_thermal_status: None,
      sensor_temperatures: vec![],
      motherboard_temperatures: vec![],
      motherboard_fan_speeds: vec![],
      power_draw: Default::default(),
      cpu_power_support: Default::default(),
      motherboard_fan_support: Default::default(),
      external_component_guidance_candidates: vec![],
    });
    let stats = t.collect_process_stats();
    assert_eq!(stats.len(), 1);
    assert!((stats[0].cpu_usage - 10.0).abs() < f32::EPSILON);
  }

  #[test]
  fn collect_process_stats_drops_invalid_run_time() {
    let mut t = ArchiveTracker::new();
    t.ingest(MetricsSnapshot {
      cpu_usage: 0.0,
      memory_usage: 0.0,
      processors_usage: vec![0.0],
      gpus: vec![],
      processes: vec![ProcessSample {
        pid: 1,
        name: "p".into(),
        cpu_usage: 50.0,
        memory_kb: 1024.0,
        // exceed 30-day cap
        run_time_secs: (60 * 60 * 24 * 31) as u64,
      }],
      cpu_temperature: None,
      cpu_package_thermal_status: None,
      sensor_temperatures: vec![],
      motherboard_temperatures: vec![],
      motherboard_fan_speeds: vec![],
      power_draw: Default::default(),
      cpu_power_support: Default::default(),
      motherboard_fan_support: Default::default(),
      external_component_guidance_candidates: vec![],
    });
    assert!(t.collect_process_stats().is_empty());
  }

  // ── collect_gpu_data ──

  #[test]
  fn collect_gpu_data_uses_name_from_map() {
    let mut t = ArchiveTracker::new();
    t.ingest(MetricsSnapshot {
      cpu_usage: 0.0,
      memory_usage: 0.0,
      processors_usage: vec![0.0],
      gpus: vec![GpuMetric {
        gpu_id: "gpu:0".into(),
        gpu_name: "RTX 4090".into(),
        gpu_usage: Some(50.0),
        gpu_temperature: Some(70.0),
        gpu_source: "Test".into(),
        gpu_dedicated_memory_usage_kb: Some(4096.0),
        gpu_cooler_level: None,
      }],
      processes: vec![],
      cpu_temperature: None,
      cpu_package_thermal_status: None,
      sensor_temperatures: vec![],
      motherboard_temperatures: vec![],
      motherboard_fan_speeds: vec![],
      power_draw: Default::default(),
      cpu_power_support: Default::default(),
      motherboard_fan_support: Default::default(),
      external_component_guidance_candidates: vec![],
    });
    let gpus = t.collect_gpu_data();
    assert_eq!(gpus.len(), 1);
    assert_eq!(gpus[0].gpu_id.as_deref(), Some("gpu:0"));
    assert_eq!(gpus[0].gpu_name, "RTX 4090");
    assert_eq!(gpus[0].usage_avg, Some(50.0));
    assert_eq!(gpus[0].temperature_max, Some(70));
    assert_eq!(gpus[0].dedicated_memory_min, Some(4096));
  }

  #[test]
  fn collect_gpu_data_aggregates_raw_same_name_histories_for_archive() {
    let mut t = ArchiveTracker::new();
    t.ingest(MetricsSnapshot {
      cpu_usage: 0.0,
      memory_usage: 0.0,
      processors_usage: vec![0.0],
      gpus: vec![
        GpuMetric {
          gpu_id: "pdh:instance:adapter-a".into(),
          gpu_name: "Intel UHD Graphics".into(),
          gpu_usage: Some(10.0),
          gpu_temperature: Some(40.0),
          gpu_source: "PDH".into(),
          gpu_dedicated_memory_usage_kb: Some(100.0),
          gpu_cooler_level: None,
        },
        GpuMetric {
          gpu_id: "pdh:instance:adapter-b".into(),
          gpu_name: "Intel UHD Graphics".into(),
          gpu_usage: Some(30.0),
          gpu_temperature: Some(60.0),
          gpu_source: "PDH".into(),
          gpu_dedicated_memory_usage_kb: Some(300.0),
          gpu_cooler_level: None,
        },
      ],
      processes: vec![],
      cpu_temperature: None,
      cpu_package_thermal_status: None,
      sensor_temperatures: vec![],
      motherboard_temperatures: vec![],
      motherboard_fan_speeds: vec![],
      power_draw: Default::default(),
      cpu_power_support: Default::default(),
      motherboard_fan_support: Default::default(),
      external_component_guidance_candidates: vec![],
    });

    t.ingest(MetricsSnapshot {
      cpu_usage: 0.0,
      memory_usage: 0.0,
      processors_usage: vec![0.0],
      gpus: vec![GpuMetric {
        gpu_id: "pdh:instance:adapter-a".into(),
        gpu_name: "Intel UHD Graphics".into(),
        gpu_usage: Some(20.0),
        gpu_temperature: Some(50.0),
        gpu_source: "PDH".into(),
        gpu_dedicated_memory_usage_kb: Some(200.0),
        gpu_cooler_level: None,
      }],
      processes: vec![],
      cpu_temperature: None,
      cpu_package_thermal_status: None,
      sensor_temperatures: vec![],
      motherboard_temperatures: vec![],
      motherboard_fan_speeds: vec![],
      power_draw: Default::default(),
      cpu_power_support: Default::default(),
      motherboard_fan_support: Default::default(),
      external_component_guidance_candidates: vec![],
    });

    let gpus = t.collect_gpu_data();
    assert_eq!(gpus.len(), 1);
    assert_eq!(gpus[0].gpu_id, None);
    assert_eq!(gpus[0].gpu_name, "Intel UHD Graphics");
    assert_eq!(gpus[0].usage_avg, Some(20.0));
    assert_eq!(gpus[0].usage_min, Some(10.0));
    assert_eq!(gpus[0].usage_max, Some(30.0));
    assert_eq!(gpus[0].temperature_avg, Some(50.0));
    assert_eq!(gpus[0].dedicated_memory_avg, Some(200));
  }

  // ── collect_ambient_data ──

  mod ambient {
    use super::*;
    use crate::infrastructure::providers::environmental::{
      AMBIENT_READING_MAX_AGE_SECONDS, EnvironmentalReading, EnvironmentalSensorProvider,
    };
    use chrono::{DateTime, Utc};

    struct StubSensor {
      source: String,
      reading: Option<EnvironmentalReading>,
    }

    impl EnvironmentalSensorProvider for StubSensor {
      fn source(&self) -> String {
        self.source.clone()
      }

      fn latest_reading(&self) -> Option<EnvironmentalReading> {
        self.reading.clone()
      }
    }

    fn tick() -> DateTime<Utc> {
      DateTime::parse_from_rfc3339("2026-08-30T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
    }

    fn sensor(
      source: &str,
      temperature: f32,
      humidity: Option<f32>,
      seconds_old: i64,
    ) -> Arc<StubSensor> {
      Arc::new(StubSensor {
        source: source.to_string(),
        reading: Some(EnvironmentalReading {
          temperature_celsius: temperature,
          humidity_percent: humidity,
          timestamp: tick() - chrono::Duration::seconds(seconds_old),
          source: source.to_string(),
        }),
      })
    }

    fn tracker(sensors: Vec<Arc<StubSensor>>) -> ArchiveTracker {
      let mut registry = EnvironmentalSensorRegistry::new();
      for sensor in sensors {
        registry.register(sensor);
      }
      ArchiveTracker::with_environmental_sensors(Arc::new(registry))
    }

    #[test]
    fn an_install_without_an_ambient_sensor_writes_no_ambient_rows() {
      let tracker = ArchiveTracker::new();
      assert!(tracker.collect_ambient_data(tick()).is_empty());
    }

    #[test]
    fn every_fresh_ambient_source_becomes_its_own_row_for_the_tick() {
      let tracker = tracker(vec![
        sensor("Living Room", 24.5, Some(48.0), 30),
        sensor("Desk", 26.0, None, 30),
      ]);

      assert_eq!(
        tracker.collect_ambient_data(tick()),
        vec![
          AmbientData {
            source: "Living Room".to_string(),
            temperature: 24.5,
            humidity: Some(48.0),
          },
          AmbientData {
            source: "Desk".to_string(),
            temperature: 26.0,
            humidity: None,
          },
        ]
      );
    }

    #[test]
    fn a_sensor_that_went_quiet_leaves_the_minute_without_an_ambient_row() {
      let tracker = tracker(vec![sensor(
        "Living Room",
        24.5,
        Some(48.0),
        AMBIENT_READING_MAX_AGE_SECONDS + 1,
      )]);

      assert!(
        tracker.collect_ambient_data(tick()).is_empty(),
        "a stale sensor must not keep writing its last value every minute"
      );
    }
  }

  #[test]
  fn collect_gpu_data_falls_back_to_id_when_name_missing() {
    // Direct construction skips the ingest path so we can simulate a
    // GPU id that has metrics but no recorded name.
    let mut t = ArchiveTracker::new();
    t.gpu_usage_histories
      .entry("anon".to_string())
      .or_default()
      .push_back(50.0);
    let gpus = t.collect_gpu_data();
    assert_eq!(gpus.len(), 1);
    assert_eq!(gpus[0].gpu_name, "anon");
  }
}
