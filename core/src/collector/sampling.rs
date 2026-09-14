//! One sampling cycle of system + GPU metrics.
//!
//! Functions in this module read sysinfo / OS-specific provider state and
//! mutate the shared [`crate::collector::history::HistoryStore`].
//! Temperature unit conversion is **not** done here — collectors always
//! publish raw °C in [`crate::models::MetricsSnapshot`]. The App-side
//! `WindowAdapter` applies the user's preferred unit before emitting the
//! Tauri `HardwareMonitorUpdate` event.

use crate::collector::HistoryStore;
use crate::models::{
  ExternalComponentGuidanceCandidate, GpuMetric, GpuSample, MetricsSnapshot,
  MotherboardSensorCollection, PowerDraw, ProcessSample, SensorTemperature,
  TemperatureSample,
};
use crate::platform::factory::PlatformHandle;
use crate::platform::traits::Platform;

pub struct SystemSample {
  pub cpu_usage: f32,
  pub memory_usage: f32,
  pub processors_usage: Vec<f32>,
  pub processes: Vec<ProcessSample>,
}

/// Run one full sampling cycle against the platform resolved once at
/// collector startup and compose the resulting [`MetricsSnapshot`].
///
/// A failed platform resolution is reported per cycle: sensor samples
/// become unavailable with the initialization error as the reason, GPU
/// samples stay empty, and power falls back to the default. Returns
/// `None` only when the system sample itself is unavailable.
pub async fn collect_snapshot(
  store: &HistoryStore,
  platform: &PlatformHandle,
) -> Option<MetricsSnapshot> {
  let system_sample = sample_system(store)?;

  let (
    gpu_samples,
    power_draw,
    cpu_power_support,
    temperature_sample,
    motherboard_collection,
  ) = match platform {
    Ok(platform) => (
      sample_gpu(store, platform.as_ref()).await,
      platform.sample_power_draw(),
      platform.cpu_power_support(),
      platform.sample_temperatures(),
      platform.sample_motherboard_sensors(),
    ),
    Err(error) => (
      Vec::new(),
      PowerDraw::default(),
      crate::models::SensorSupport::Unknown,
      TemperatureSample::unavailable(error.to_string()),
      MotherboardSensorCollection::unavailable(error.to_string()),
    ),
  };

  Some(build_metrics_snapshot(
    &system_sample,
    &gpu_samples,
    power_draw,
    cpu_power_support,
    &temperature_sample,
    &motherboard_collection,
    &motherboard_collection.guidance_candidates,
  ))
}

/// Run one CPU / memory / process refresh and append samples to the
/// store's history rings.
pub fn sample_system(store: &HistoryStore) -> Option<SystemSample> {
  let result = store.system().lock().ok().map(|mut sys| {
    sys.refresh_cpu_usage();
    sys.refresh_memory_specifics(sysinfo::MemoryRefreshKind::nothing().with_ram());
    // Keep sysinfo's default task enumeration so Linux process membership
    // remains compatible with refresh_all().
    sys.refresh_processes_specifics(
      sysinfo::ProcessesToUpdate::All,
      true,
      sysinfo::ProcessRefreshKind::nothing()
        .with_cpu()
        .with_memory(),
    );

    let cpu_usage = calculate_average_cpu_usage(sys.cpus());
    let memory_usage =
      calculate_memory_usage_percentage(sys.used_memory(), sys.total_memory());
    let processors_usage: Vec<f32> = sys.cpus().iter().map(|c| c.cpu_usage()).collect();

    let processes: Vec<ProcessSample> = sys
      .processes()
      .iter()
      .map(|(pid, process)| ProcessSample {
        pid: pid.as_u32(),
        name: process.name().to_string_lossy().into_owned(),
        cpu_usage: process.cpu_usage(),
        memory_kb: process.memory() as f32 / 1024.0,
        run_time_secs: process.run_time(),
      })
      .collect();

    let process_history_input: Vec<_> = processes
      .iter()
      .map(|p| (sysinfo::Pid::from_u32(p.pid), p.cpu_usage, p.memory_kb))
      .collect();

    (
      cpu_usage,
      memory_usage,
      processors_usage,
      processes,
      process_history_input,
    )
  });

  let (cpu_usage, memory_usage, processors_usage, processes, process_history_input) =
    result?;

  store.push_cpu_history(cpu_usage);
  store.push_memory_history(memory_usage);
  store.update_process_histories(&process_history_input);

  Some(SystemSample {
    cpu_usage,
    memory_usage,
    processors_usage,
    processes,
  })
}

fn calculate_average_cpu_usage(cpus: &[sysinfo::Cpu]) -> f32 {
  match cpus.len() {
    0 => 0.0,
    len => (cpus.iter().map(|cpu| cpu.cpu_usage()).sum::<f32>() / len as f32).round(),
  }
}

fn calculate_memory_usage_percentage(used: u64, total: u64) -> f32 {
  match total {
    0 => 0.0,
    total => ((used as f64 / total as f64) * 100.0).round() as f32,
  }
}

pub async fn sample_gpu(store: &HistoryStore, platform: &dyn Platform) -> Vec<GpuSample> {
  let gpu_metrics = platform.sample_gpus().await;

  if !gpu_metrics.is_empty() {
    store.update_gpu_histories(&gpu_metrics);
  }

  gpu_metrics
}

/// Build the per-GPU metrics carried in [`MetricsSnapshot`]. Temperatures
/// are forwarded as raw °C; the App-side adapter applies the user's
/// preferred unit.
fn build_gpu_metrics(gpu_samples: &[GpuSample]) -> Vec<GpuMetric> {
  gpu_samples
    .iter()
    .map(|s| GpuMetric {
      gpu_id: s.gpu_id.clone(),
      gpu_name: s.name.clone(),
      gpu_usage: s.usage.map(|u| u.round()),
      gpu_temperature: s.temperature.map(|t| t.round()),
      gpu_source: s.source.clone(),
      gpu_dedicated_memory_usage_kb: s.dedicated_memory_kb,
      gpu_cooler_level: s.cooler_level,
    })
    .collect()
}

/// Compose a [`MetricsSnapshot`] from one cycle's system + GPU +
/// temperature samples. Temperatures are forwarded as raw °C (rounded);
/// the App-side adapter applies the user's preferred unit.
pub fn build_metrics_snapshot(
  system_sample: &SystemSample,
  gpu_samples: &[GpuSample],
  power_draw: PowerDraw,
  cpu_power_support: crate::models::SensorSupport,
  temperature_sample: &TemperatureSample,
  motherboard_collection: &MotherboardSensorCollection,
  motherboard_guidance_candidates: &[ExternalComponentGuidanceCandidate],
) -> MetricsSnapshot {
  let motherboard_sample = &motherboard_collection.sample;
  MetricsSnapshot {
    cpu_usage: system_sample.cpu_usage,
    memory_usage: system_sample.memory_usage,
    processors_usage: system_sample.processors_usage.clone(),
    gpus: build_gpu_metrics(gpu_samples),
    power_draw,
    cpu_power_support,
    processes: system_sample.processes.clone(),
    cpu_temperature: temperature_sample.cpu_temperature.map(|t| t.round()),
    cpu_package_thermal_status: temperature_sample.cpu_package_thermal_status,
    sensor_temperatures: temperature_sample
      .sensor_temperatures
      .iter()
      .map(|s| SensorTemperature {
        name: s.name.clone(),
        temperature: s.temperature.round(),
      })
      .collect(),
    motherboard_temperatures: motherboard_sample
      .temperatures
      .iter()
      .map(|s| crate::models::MotherboardTemperature {
        name: s.name.clone(),
        temperature: s.temperature.round(),
        source: s.source.clone(),
      })
      .collect(),
    motherboard_fan_speeds: motherboard_sample.fan_speeds.clone(),
    motherboard_fan_support: motherboard_collection.fan_support,
    external_component_guidance_candidates: temperature_sample
      .guidance_candidates
      .iter()
      .chain(motherboard_guidance_candidates.iter())
      .cloned()
      .collect(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::enums::error::PlatformError;
  use crate::models::hardware::{
    GpuMemoryUsage, GraphicInfo, MemoryInfo, MotherboardInfo, NameValue, NetworkInfo,
    SuperIoChipIdDiagnostics,
  };
  use crate::models::{MotherboardSensorSample, SensorAvailability};
  use crate::platform::traits::{
    ElevatedProcessRun, ExternalComponentSetupPlatform, GpuPlatform, GpuUsageRaw,
    MemoryPlatform, MotherboardPlatform, NetworkPlatform, ProcessElevationPlatform,
    SensorPlatform, SuperIoPlatform,
  };
  use async_trait::async_trait;
  use std::sync::Arc;

  /// Deterministic Platform used to exercise the collector cycle without
  /// touching OS providers — the injection seam introduced by the shared
  /// platform handle.
  struct FakePlatform;

  #[async_trait]
  impl MemoryPlatform for FakePlatform {
    async fn get_memory_info(&self) -> Result<MemoryInfo, PlatformError> {
      Err(PlatformError::unsupported("fake"))
    }

    async fn get_memory_info_detail(&self) -> Result<MemoryInfo, PlatformError> {
      Err(PlatformError::unsupported("fake"))
    }
  }

  #[async_trait]
  impl GpuPlatform for FakePlatform {
    async fn get_gpu_usage(&self) -> Result<GpuUsageRaw, PlatformError> {
      Err(PlatformError::unsupported("fake"))
    }

    async fn get_gpu_temperature(&self) -> Result<Vec<NameValue>, PlatformError> {
      Err(PlatformError::unsupported("fake"))
    }

    async fn get_gpu_info(&self) -> Result<Vec<GraphicInfo>, PlatformError> {
      Ok(Vec::new())
    }

    async fn get_gpu_memory_usage(
      &self,
    ) -> Result<Option<GpuMemoryUsage>, PlatformError> {
      Ok(None)
    }

    async fn sample_gpus(&self) -> Vec<GpuSample> {
      vec![GpuSample {
        gpu_id: "fake:0".to_string(),
        name: "Fake GPU".to_string(),
        usage: Some(12.4),
        temperature: Some(55.6),
        dedicated_memory_kb: None,
        cooler_level: None,
        source: "Fake".to_string(),
      }]
    }

    fn sample_power_draw(&self) -> PowerDraw {
      PowerDraw {
        cpu_watts: Some(1.5),
        gpu_watts: Some(2.5),
        ane_watts: None,
        package_watts: Some(4.0),
      }
    }

    fn cpu_power_support(&self) -> crate::models::SensorSupport {
      crate::models::SensorSupport::Supported
    }
  }

  impl NetworkPlatform for FakePlatform {
    fn get_network_info(&self) -> Result<Vec<NetworkInfo>, PlatformError> {
      Ok(Vec::new())
    }
  }

  #[async_trait]
  impl MotherboardPlatform for FakePlatform {
    async fn get_motherboard_info(&self) -> Result<MotherboardInfo, PlatformError> {
      Err(PlatformError::unsupported("fake"))
    }
  }

  impl SuperIoPlatform for FakePlatform {
    fn get_super_io_chip_id_diagnostics(&self) -> SuperIoChipIdDiagnostics {
      SuperIoChipIdDiagnostics::unsupported_platform()
    }
  }

  impl SensorPlatform for FakePlatform {
    fn sample_temperatures(&self) -> TemperatureSample {
      TemperatureSample {
        cpu_temperature: Some(42.4),
        ..TemperatureSample::default()
      }
    }

    fn sample_motherboard_sensors(&self) -> MotherboardSensorCollection {
      MotherboardSensorCollection::unsupported("fake fan path")
    }
  }

  impl ProcessElevationPlatform for FakePlatform {
    fn is_process_elevated(&self) -> Result<bool, PlatformError> {
      Ok(false)
    }

    fn relaunch_current_process_elevated(&self) -> Result<(), PlatformError> {
      Err(PlatformError::unsupported("fake"))
    }
  }

  impl ExternalComponentSetupPlatform for FakePlatform {
    fn external_component_setup_status(
      &self,
      plan: &crate::external_component_setup::ExternalComponentSetupPlan,
    ) -> crate::external_component_setup::ExternalComponentSetupStatus {
      crate::external_component_setup::ExternalComponentSetupStatus::unsupported_platform(
        plan,
      )
    }

    fn run_external_component_setup(
      &self,
      plan: &crate::external_component_setup::ExternalComponentSetupPlan,
    ) -> crate::external_component_setup::ExternalComponentSetupResult {
      crate::external_component_setup::ExternalComponentSetupResult::failed(
        plan.component,
        crate::external_component_setup::SetupFailureStage::Other,
        "fake",
      )
    }

    fn run_current_executable_elevated(
      &self,
      _args: &[String],
    ) -> Result<ElevatedProcessRun, PlatformError> {
      Err(PlatformError::unsupported("fake"))
    }
  }

  impl Platform for FakePlatform {}

  #[tokio::test(flavor = "current_thread")]
  async fn collect_snapshot_uses_the_injected_platform() {
    let store = HistoryStore::new();
    let platform: PlatformHandle = Ok(Arc::new(FakePlatform));

    let snapshot = collect_snapshot(&store, &platform)
      .await
      .expect("system sample must be available in tests");

    assert_eq!(snapshot.gpus.len(), 1);
    assert_eq!(snapshot.gpus[0].gpu_id, "fake:0");
    assert_eq!(snapshot.gpus[0].gpu_usage, Some(12.0));
    assert_eq!(snapshot.gpus[0].gpu_temperature, Some(56.0));
    assert_eq!(snapshot.cpu_temperature, Some(42.0));
    assert_eq!(snapshot.power_draw.package_watts, Some(4.0));
    assert_eq!(
      snapshot.cpu_power_support,
      crate::models::SensorSupport::Supported
    );
    assert_eq!(
      snapshot.motherboard_fan_support,
      crate::models::SensorSupport::Unsupported
    );
    // The fake GPU sample also landed in the history rings.
    assert_eq!(store.gpu_history("fake:0", 60).len(), 1);
  }

  #[tokio::test(flavor = "current_thread")]
  async fn collect_snapshot_degrades_when_the_platform_is_unavailable() {
    let store = HistoryStore::new();
    let platform: PlatformHandle = Err(PlatformError::initialization_failed("boom"));

    let snapshot = collect_snapshot(&store, &platform)
      .await
      .expect("system sample must be available in tests");

    assert!(snapshot.gpus.is_empty());
    assert_eq!(snapshot.power_draw, PowerDraw::default());
    assert_eq!(snapshot.cpu_temperature, None);
    assert_eq!(
      snapshot.cpu_power_support,
      crate::models::SensorSupport::Unknown
    );
    assert_eq!(
      snapshot.motherboard_fan_support,
      crate::models::SensorSupport::Unknown
    );
    assert!(snapshot.sensor_temperatures.is_empty());
    assert!(snapshot.motherboard_temperatures.is_empty());
  }

  fn make_sample(
    gpu_id: &str,
    name: &str,
    usage: Option<f32>,
    temp: Option<f32>,
  ) -> GpuSample {
    GpuSample {
      gpu_id: gpu_id.to_string(),
      name: name.to_string(),
      usage,
      temperature: temp,
      dedicated_memory_kb: Some(4096.0),
      cooler_level: Some(60),
      source: "Test".to_string(),
    }
  }

  // ── calculate_memory_usage_percentage ──

  #[test]
  fn memory_usage_percentage_total_zero() {
    assert_eq!(calculate_memory_usage_percentage(1000, 0), 0.0);
  }

  #[test]
  fn memory_usage_percentage_half_used() {
    assert_eq!(calculate_memory_usage_percentage(500, 1000), 50.0);
  }

  #[test]
  fn memory_usage_percentage_fully_used() {
    assert_eq!(calculate_memory_usage_percentage(1000, 1000), 100.0);
  }

  #[test]
  fn memory_usage_percentage_zero_used() {
    assert_eq!(calculate_memory_usage_percentage(0, 1000), 0.0);
  }

  #[test]
  fn memory_usage_percentage_rounding() {
    assert_eq!(calculate_memory_usage_percentage(333, 1000), 33.0);
  }

  #[test]
  fn memory_usage_percentage_large_values() {
    assert_eq!(
      calculate_memory_usage_percentage(16_000_000_000, 32_000_000_000),
      50.0
    );
  }

  #[test]
  fn sample_system_refreshes_required_system_and_process_data() {
    let store = HistoryStore::new();
    let current_pid =
      sysinfo::get_current_pid().expect("current process should have a PID");
    const SENTINEL_CPU_USAGE: f32 = 321.0;
    const SENTINEL_MEMORY_KB: f32 = 1_048_576.0;

    store.update_process_histories(&[(
      current_pid,
      SENTINEL_CPU_USAGE,
      SENTINEL_MEMORY_KB,
    )]);

    {
      let mut system = store.system().lock().unwrap();
      *system = sysinfo::System::new();
      assert!(system.cpus().is_empty());
      assert_eq!(system.total_memory(), 0);
      assert!(system.processes().is_empty());
    }

    let sample = sample_system(&store).expect("system lock should be available");
    let system = store.system().lock().unwrap();

    assert!(!sample.processors_usage.is_empty());
    assert!(system.total_memory() > 0);
    assert_eq!(sample.cpu_usage, calculate_average_cpu_usage(system.cpus()));
    assert_eq!(
      sample.processors_usage,
      system
        .cpus()
        .iter()
        .map(|cpu| cpu.cpu_usage())
        .collect::<Vec<_>>()
    );
    assert_eq!(
      sample.memory_usage,
      calculate_memory_usage_percentage(system.used_memory(), system.total_memory())
    );

    let process = system
      .process(current_pid)
      .expect("targeted refresh should include the current process");
    let process_sample = sample
      .processes
      .iter()
      .find(|sample| sample.pid == current_pid.as_u32())
      .expect("system sample should include the current process");

    assert!(!process_sample.name.is_empty());
    assert_eq!(
      process_sample.name,
      process.name().to_string_lossy().into_owned()
    );
    assert_eq!(process_sample.cpu_usage, process.cpu_usage());
    assert_eq!(process_sample.memory_kb, process.memory() as f32 / 1024.0);
    assert_eq!(process_sample.run_time_secs, process.run_time());
    drop(system);

    assert_eq!(store.cpu_history(1), vec![sample.cpu_usage]);
    assert_eq!(store.memory_history(1), vec![sample.memory_usage]);
    let process_info = store
      .process_list()
      .into_iter()
      .find(|process| process.pid == current_pid.as_u32() as i32)
      .expect("process list should include the current process");
    assert_eq!(process_info.name, process_sample.name);
    assert_eq!(
      process_info.cpu_usage,
      crate::utils::rounding::round1(
        ((SENTINEL_CPU_USAGE + process_sample.cpu_usage) / 2.0)
          / sample.processors_usage.len() as f32
      )
    );
    assert_eq!(
      process_info.memory_usage,
      crate::utils::rounding::round1(
        ((SENTINEL_MEMORY_KB + process_sample.memory_kb) / 2.0) / 1024.0
      )
    );
  }

  // ── build_gpu_metrics ──

  #[test]
  fn build_gpus_from_empty_samples() {
    let result = build_gpu_metrics(&[]);
    assert!(result.is_empty());
  }

  #[test]
  fn build_gpus_from_single_sample() {
    let samples = vec![make_sample("gpu:0", "RTX 4090", Some(75.3), Some(65.0))];
    let result = build_gpu_metrics(&samples);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].gpu_id, "gpu:0");
    assert_eq!(result[0].gpu_name, "RTX 4090");
    assert_eq!(result[0].gpu_usage, Some(75.0)); // rounded
    assert_eq!(result[0].gpu_temperature, Some(65.0));
    assert_eq!(result[0].gpu_dedicated_memory_usage_kb, Some(4096.0));
    assert_eq!(result[0].gpu_cooler_level, Some(60));
  }

  #[test]
  fn build_gpus_passes_through_celsius_unchanged() {
    let samples = vec![make_sample("gpu:0", "GPU", None, Some(100.0))];
    let result = build_gpu_metrics(&samples);
    // Core publishes raw °C; the App-side adapter does any conversion.
    assert_eq!(result[0].gpu_temperature, Some(100.0));
  }

  #[test]
  fn build_gpus_temperature_none_preserved() {
    let samples = vec![make_sample("gpu:0", "GPU", Some(50.0), None)];
    let result = build_gpu_metrics(&samples);
    assert!(result[0].gpu_temperature.is_none());
  }

  #[test]
  fn build_gpus_usage_rounded() {
    let samples = vec![make_sample("gpu:0", "GPU", Some(33.7), None)];
    let result = build_gpu_metrics(&samples);
    assert_eq!(result[0].gpu_usage, Some(34.0));
  }

  #[test]
  fn build_gpus_from_multiple_samples() {
    let samples = vec![
      make_sample("pci:0:2.0", "RTX 4090", Some(80.0), Some(70.0)),
      make_sample("pci:0:3.0", "RX 7900 XTX", Some(50.0), Some(60.0)),
    ];
    let result = build_gpu_metrics(&samples);
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].gpu_id, "pci:0:2.0");
    assert_eq!(result[1].gpu_id, "pci:0:3.0");
  }

  // ── build_metrics_snapshot ──

  #[test]
  fn snapshot_carries_system_fields_and_gpus() {
    let sys = SystemSample {
      cpu_usage: 12.5,
      memory_usage: 67.0,
      processors_usage: vec![10.0, 20.0, 30.0, 40.0],
      processes: vec![ProcessSample {
        pid: 1,
        name: "init".into(),
        cpu_usage: 0.0,
        memory_kb: 1024.0,
        run_time_secs: 60,
      }],
    };
    let gpus = vec![make_sample("gpu:0", "RTX 4090", Some(50.0), Some(70.0))];
    let snap = build_metrics_snapshot(
      &sys,
      &gpus,
      PowerDraw::default(),
      crate::models::SensorSupport::Unknown,
      &TemperatureSample::default(),
      &MotherboardSensorCollection::default(),
      &[],
    );
    assert_eq!(snap.cpu_usage, 12.5);
    assert_eq!(snap.memory_usage, 67.0);
    assert_eq!(snap.processors_usage, vec![10.0, 20.0, 30.0, 40.0]);
    assert_eq!(snap.gpus.len(), 1);
    assert_eq!(snap.gpus[0].gpu_id, "gpu:0");
    assert_eq!(snap.processes.len(), 1);
    assert_eq!(snap.processes[0].pid, 1);
    assert_eq!(snap.processes[0].name, "init");
    assert_eq!(snap.cpu_temperature, None);
    assert!(snap.sensor_temperatures.is_empty());
  }

  #[test]
  fn snapshot_carries_live_power_without_filling_missing_components() {
    let snapshot = build_metrics_snapshot(
      &SystemSample {
        cpu_usage: 0.0,
        memory_usage: 0.0,
        processors_usage: vec![],
        processes: vec![],
      },
      &[make_sample("gpu:0", "GPU", Some(50.0), None)],
      PowerDraw {
        cpu_watts: Some(10.1),
        gpu_watts: Some(2.2),
        ..Default::default()
      },
      crate::models::SensorSupport::Supported,
      &TemperatureSample::default(),
      &MotherboardSensorCollection::default(),
      &[],
    );

    assert_eq!(snapshot.power_draw.cpu_watts, Some(10.1));
    assert_eq!(snapshot.power_draw.gpu_watts, Some(2.2));
    assert_eq!(snapshot.power_draw.ane_watts, None);
    assert_eq!(snapshot.power_draw.package_watts, None);
  }

  #[test]
  fn snapshot_rounds_cpu_and_sensor_temperatures() {
    let sys = SystemSample {
      cpu_usage: 0.0,
      memory_usage: 0.0,
      processors_usage: vec![],
      processes: vec![],
    };
    let temps = TemperatureSample {
      cpu_temperature: Some(49.95),
      cpu_package_thermal_status: Some(crate::models::CpuPackageThermalStatus {
        thermal_status: false,
        prochot_or_forcepr_asserted: true,
        power_limitation_status: true,
      }),
      sensor_temperatures: vec![
        SensorTemperature {
          name: "CPUZ".into(),
          temperature: 49.95,
        },
        SensorTemperature {
          name: "TZ01".into(),
          temperature: 40.2,
        },
      ],
      availability: SensorAvailability::Available,
      guidance_candidates: Vec::new(),
    };
    let snap = build_metrics_snapshot(
      &sys,
      &[],
      PowerDraw::default(),
      crate::models::SensorSupport::Unknown,
      &temps,
      &MotherboardSensorCollection::default(),
      &[],
    );
    assert_eq!(snap.cpu_temperature, Some(50.0));
    assert_eq!(
      snap.cpu_package_thermal_status,
      temps.cpu_package_thermal_status
    );
    assert_eq!(
      snap.sensor_temperatures,
      vec![
        SensorTemperature {
          name: "CPUZ".into(),
          temperature: 50.0,
        },
        SensorTemperature {
          name: "TZ01".into(),
          temperature: 40.0,
        },
      ]
    );
  }

  #[test]
  fn snapshot_carries_motherboard_sensors() {
    let sys = SystemSample {
      cpu_usage: 0.0,
      memory_usage: 0.0,
      processors_usage: vec![],
      processes: vec![],
    };
    let motherboard_sample = MotherboardSensorSample {
      temperatures: vec![crate::models::MotherboardTemperature {
        name: "SYSTIN".into(),
        temperature: 39.6,
        source: "NCT6799D / Super I/O".into(),
      }],
      fan_speeds: vec![crate::models::MotherboardFanSpeed {
        name: "Fan 1".into(),
        rpm: Some(0),
        status: crate::models::FanSpeedStatus::Inactive,
        source: "NCT6799D / Super I/O".into(),
      }],
    };
    let motherboard_collection = MotherboardSensorCollection {
      sample: motherboard_sample.clone(),
      availability: SensorAvailability::Available,
      fan_support: crate::models::SensorSupport::Supported,
      guidance_candidates: Vec::new(),
    };

    let snap = build_metrics_snapshot(
      &sys,
      &[],
      PowerDraw::default(),
      crate::models::SensorSupport::Unknown,
      &TemperatureSample::default(),
      &motherboard_collection,
      &[],
    );

    assert_eq!(snap.motherboard_temperatures.len(), 1);
    assert_eq!(snap.motherboard_temperatures[0].name, "SYSTIN");
    assert_eq!(snap.motherboard_temperatures[0].temperature, 40.0);
    assert_eq!(
      snap.motherboard_temperatures[0].source,
      "NCT6799D / Super I/O"
    );
    assert_eq!(snap.motherboard_fan_speeds, motherboard_sample.fan_speeds);
  }

  #[test]
  fn snapshot_carries_external_component_guidance_candidates() {
    let sys = SystemSample {
      cpu_usage: 0.0,
      memory_usage: 0.0,
      processors_usage: vec![],
      processes: vec![],
    };
    let candidate = ExternalComponentGuidanceCandidate::pawnio_cpu_package_temperature(
      "PawnIOLib.dll not found".to_string(),
    );
    let temps = TemperatureSample {
      cpu_temperature: None,
      cpu_package_thermal_status: None,
      sensor_temperatures: vec![],
      availability: SensorAvailability::unavailable(
        "PawnIO unavailable; ACPI thermal zones unavailable".to_string(),
      ),
      guidance_candidates: vec![candidate.clone()],
    };

    let snap = build_metrics_snapshot(
      &sys,
      &[],
      PowerDraw::default(),
      crate::models::SensorSupport::Unknown,
      &temps,
      &MotherboardSensorCollection::default(),
      &[],
    );

    assert_eq!(snap.external_component_guidance_candidates, vec![candidate]);
  }

  #[test]
  fn snapshot_carries_motherboard_external_component_guidance_candidates() {
    let sys = SystemSample {
      cpu_usage: 0.0,
      memory_usage: 0.0,
      processors_usage: vec![],
      processes: vec![],
    };
    let candidate = ExternalComponentGuidanceCandidate::pawnio_motherboard_sensors(
      "pawnio_open failed: 0x80070005".to_string(),
    );

    let snap = build_metrics_snapshot(
      &sys,
      &[],
      PowerDraw::default(),
      crate::models::SensorSupport::Unknown,
      &TemperatureSample::default(),
      &MotherboardSensorCollection::default(),
      std::slice::from_ref(&candidate),
    );

    assert_eq!(snap.external_component_guidance_candidates, vec![candidate]);
  }
}
