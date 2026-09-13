use std::sync::{Arc, Mutex};

use hardviz_core::models::{
  GpuMetric, MetricsSnapshot, MotherboardFanSpeed, MotherboardTemperature,
  SensorTemperature,
};
use tauri::Manager as _;
use tokio::sync::broadcast::{Receiver, error::RecvError};
use tokio::sync::watch;

use crate::commands::settings;
use crate::enums::settings::TemperatureUnit;
use crate::log_warn;
use crate::models::hardware::{
  GpuMonitorData, HardwareMonitorUpdate, MotherboardFanSpeedValue,
  MotherboardTemperatureValue, NameValue,
};
use tauri_specta::Event as _;

/// Subscribes to the in-process [`hardviz_core::event_bus::EventBus`] and
/// forwards each [`MetricsSnapshot`] to the main window as the existing
/// [`HardwareMonitorUpdate`] Tauri event. This is the only place that
/// translates Core events into a Tauri emit. It also applies the user's
/// preferred temperature unit (`Celsius` / `Fahrenheit`) - the Core
/// collector always publishes raw °C, and presentation conversion lives
/// here at the App-side boundary.
pub struct WindowAdapter {
  handle: tauri::async_runtime::JoinHandle<()>,
  stop_tx: watch::Sender<bool>,
  latest_snapshot: LatestWindowSnapshot,
}

#[derive(Clone, Default)]
struct LatestWindowSnapshot {
  inner: Arc<Mutex<Option<MetricsSnapshot>>>,
}

impl LatestWindowSnapshot {
  fn store(&self, snapshot: MetricsSnapshot) {
    if let Ok(mut latest) = self.inner.lock() {
      latest.replace(snapshot);
    }
  }

  fn load(&self) -> Option<MetricsSnapshot> {
    self.inner.lock().ok().and_then(|latest| latest.clone())
  }
}

impl WindowAdapter {
  pub fn setup(app_handle: tauri::AppHandle, mut rx: Receiver<MetricsSnapshot>) -> Self {
    let (stop_tx, mut stop_rx) = watch::channel(false);
    let latest_snapshot = LatestWindowSnapshot::default();
    let latest_for_task = latest_snapshot.clone();

    let handle = tauri::async_runtime::spawn(async move {
      loop {
        tokio::select! {
          result = rx.recv() => match result {
            Ok(snapshot) => {
              let snapshot = to_window_snapshot(snapshot);
              latest_for_task.store(snapshot.clone());
              emit_snapshot(&app_handle, snapshot);
            }
            Err(RecvError::Lagged(skipped)) => {
              log_warn!(
                &format!("WindowAdapter lagged, dropped {skipped} snapshot(s)"),
                "adapters::window",
                None::<&str>
              );
            }
            Err(RecvError::Closed) => break,
          },
          changed = stop_rx.changed() => {
            if changed.is_err() || *stop_rx.borrow() {
              break;
            }
          }
        }
      }
    });

    Self {
      handle,
      stop_tx,
      latest_snapshot,
    }
  }

  pub fn emit_latest_if_visible(&self, app_handle: &tauri::AppHandle) {
    if let Some(snapshot) = self.latest_snapshot.load() {
      emit_snapshot(app_handle, snapshot);
    }
  }

  pub fn latest_external_component_guidance_candidates(
    &self,
  ) -> Vec<hardviz_core::models::ExternalComponentGuidanceCandidate> {
    self
      .latest_snapshot
      .load()
      .map(|snapshot| snapshot.external_component_guidance_candidates)
      .unwrap_or_default()
  }

  pub async fn terminate(self) {
    let _ = self.stop_tx.send(true);
    let _ = self.handle.await;
  }
}

fn to_window_snapshot(mut snapshot: MetricsSnapshot) -> MetricsSnapshot {
  snapshot.processes.clear();
  snapshot
}

fn emit_snapshot(app_handle: &tauri::AppHandle, snapshot: MetricsSnapshot) {
  if !should_emit_for_main_window(main_window_state(app_handle)) {
    return;
  }

  let temp_unit = current_temperature_unit(app_handle);
  let payload = to_hardware_monitor_update(snapshot, &temp_unit);
  if let Err(e) = payload.emit(app_handle) {
    log_warn!(
      &format!("failed to emit HardwareMonitorUpdate event: {e}"),
      "adapters::window",
      None::<&str>
    );
  }
}

#[derive(Clone, Copy)]
struct MainWindowState {
  is_visible: bool,
  is_minimized: bool,
}

fn main_window_state(app_handle: &tauri::AppHandle) -> Option<MainWindowState> {
  let window = app_handle.get_webview_window("main")?;
  Some(MainWindowState {
    is_visible: window.is_visible().ok()?,
    is_minimized: window.is_minimized().ok()?,
  })
}

fn should_emit_for_main_window(state: Option<MainWindowState>) -> bool {
  state.is_some_and(|state| state.is_visible && !state.is_minimized)
}

/// Read the user's preferred temperature unit from `settings::AppState`.
/// Defaults to Celsius if the state hasn't been registered yet (early
/// startup) or if the settings mutex is poisoned. We don't want a
/// poisoned lock to bring down the entire snapshot-forwarding loop.
fn current_temperature_unit(app_handle: &tauri::AppHandle) -> TemperatureUnit {
  app_handle
    .try_state::<settings::AppState>()
    .and_then(|state| {
      state
        .settings
        .lock()
        .ok()
        .map(|s| s.temperature_unit.clone())
    })
    .unwrap_or(TemperatureUnit::Celsius)
}

fn to_hardware_monitor_update(
  snapshot: MetricsSnapshot,
  temp_unit: &TemperatureUnit,
) -> HardwareMonitorUpdate {
  let power_draw = snapshot.power_draw;
  HardwareMonitorUpdate {
    cpu_usage: snapshot.cpu_usage,
    memory_usage: snapshot.memory_usage,
    processors_usage: snapshot.processors_usage,
    cpu_power_watts: power_draw.cpu_watts,
    gpu_power_watts: power_draw.gpu_watts,
    ane_power_watts: power_draw.ane_watts,
    package_power_watts: power_draw.package_watts,
    cpu_power_support: snapshot.cpu_power_support.into(),
    gpus: snapshot
      .gpus
      .into_iter()
      .map(|g| to_gpu_monitor_data(g, temp_unit))
      .collect(),
    cpu_temperature: snapshot
      .cpu_temperature
      .map(|t| convert_temperature(t, temp_unit)),
    sensor_temperatures: snapshot
      .sensor_temperatures
      .into_iter()
      .map(|s| to_sensor_name_value(s, temp_unit))
      .collect(),
    motherboard_temperatures: snapshot
      .motherboard_temperatures
      .into_iter()
      .map(|s| to_motherboard_temperature_value(s, temp_unit))
      .collect(),
    motherboard_fan_speeds: snapshot
      .motherboard_fan_speeds
      .into_iter()
      .map(to_motherboard_fan_speed_value)
      .collect(),
    motherboard_fan_support: snapshot.motherboard_fan_support.into(),
  }
}

fn to_sensor_name_value(
  sensor: SensorTemperature,
  temp_unit: &TemperatureUnit,
) -> NameValue {
  NameValue {
    name: sensor.name,
    value: convert_temperature(sensor.temperature, temp_unit) as i32,
  }
}

fn to_gpu_monitor_data(g: GpuMetric, temp_unit: &TemperatureUnit) -> GpuMonitorData {
  GpuMonitorData {
    gpu_id: g.gpu_id,
    gpu_name: g.gpu_name,
    gpu_usage: g.gpu_usage,
    gpu_temperature: g.gpu_temperature.map(|t| convert_temperature(t, temp_unit)),
    gpu_source: g.gpu_source,
    gpu_dedicated_memory_usage_kb: g.gpu_dedicated_memory_usage_kb,
    gpu_cooler_level: g.gpu_cooler_level,
  }
}

/// Convert raw °C from Core into the user's preferred unit, rounded.
fn to_motherboard_temperature_value(
  sensor: MotherboardTemperature,
  temp_unit: &TemperatureUnit,
) -> MotherboardTemperatureValue {
  MotherboardTemperatureValue {
    name: sensor.name,
    value: convert_temperature(sensor.temperature, temp_unit) as i32,
    source: sensor.source,
  }
}

fn to_motherboard_fan_speed_value(fan: MotherboardFanSpeed) -> MotherboardFanSpeedValue {
  MotherboardFanSpeedValue {
    name: fan.name,
    rpm: fan.rpm,
    status: fan.status.into(),
    source: fan.source,
  }
}

/// Convert raw degrees C from Core into the user's preferred unit, rounded.
fn convert_temperature(celsius: f32, unit: &TemperatureUnit) -> f32 {
  match unit {
    TemperatureUnit::Celsius => celsius.round(),
    TemperatureUnit::Fahrenheit => (celsius * 9.0 / 5.0 + 32.0).round(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn make_metric(gpu_id: &str, name: &str) -> GpuMetric {
    GpuMetric {
      gpu_id: gpu_id.to_string(),
      gpu_name: name.to_string(),
      gpu_usage: Some(60.0),
      gpu_temperature: Some(55.0),
      gpu_source: "Test".to_string(),
      gpu_dedicated_memory_usage_kb: Some(1024.0),
      gpu_cooler_level: Some(40),
    }
  }

  fn make_snapshot(cpu_usage: f32) -> MetricsSnapshot {
    MetricsSnapshot {
      cpu_usage,
      memory_usage: 67.0,
      processors_usage: vec![10.0, 20.0],
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

  #[test]
  fn translation_preserves_top_level_fields() {
    let snap = MetricsSnapshot {
      cpu_usage: 12.5,
      memory_usage: 67.0,
      processors_usage: vec![10.0, 20.0, 30.0, 40.0],
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
    };
    let update = to_hardware_monitor_update(snap, &TemperatureUnit::Celsius);
    assert_eq!(update.cpu_usage, 12.5);
    assert_eq!(update.memory_usage, 67.0);
    assert_eq!(update.processors_usage, vec![10.0, 20.0, 30.0, 40.0]);
    assert!(update.gpus.is_empty());
    assert!(update.cpu_temperature.is_none());
    assert!(update.sensor_temperatures.is_empty());
  }

  #[test]
  fn translation_carries_nullable_power_watts_without_unit_conversion() {
    let mut snapshot = make_snapshot(0.0);
    snapshot.power_draw.cpu_watts = Some(10.1);
    snapshot.power_draw.gpu_watts = Some(2.2);
    let update = to_hardware_monitor_update(snapshot, &TemperatureUnit::Fahrenheit);

    assert_eq!(update.cpu_power_watts, Some(10.1));
    assert_eq!(update.gpu_power_watts, Some(2.2));
    assert_eq!(update.ane_power_watts, None);
    assert_eq!(update.package_power_watts, None);
  }

  #[test]
  fn translation_preserves_per_gpu_fields_in_celsius() {
    let snap = MetricsSnapshot {
      cpu_usage: 0.0,
      memory_usage: 0.0,
      processors_usage: vec![],
      gpus: vec![
        make_metric("pci:0:2.0", "RTX 4090"),
        make_metric("pci:0:3.0", "RX 7900 XTX"),
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
    };
    let update = to_hardware_monitor_update(snap, &TemperatureUnit::Celsius);
    assert_eq!(update.gpus.len(), 2);
    assert_eq!(update.gpus[0].gpu_id, "pci:0:2.0");
    assert_eq!(update.gpus[0].gpu_name, "RTX 4090");
    assert_eq!(update.gpus[0].gpu_usage, Some(60.0));
    assert_eq!(update.gpus[0].gpu_temperature, Some(55.0));
    assert_eq!(update.gpus[0].gpu_source, "Test");
    assert_eq!(update.gpus[0].gpu_dedicated_memory_usage_kb, Some(1024.0));
    assert_eq!(update.gpus[0].gpu_cooler_level, Some(40));
    assert_eq!(update.gpus[1].gpu_id, "pci:0:3.0");
  }

  #[test]
  fn temperature_converted_to_fahrenheit() {
    let snap = MetricsSnapshot {
      cpu_usage: 0.0,
      memory_usage: 0.0,
      processors_usage: vec![],
      gpus: vec![GpuMetric {
        gpu_id: "x".into(),
        gpu_name: "x".into(),
        gpu_usage: None,
        gpu_temperature: Some(100.0),
        gpu_source: "Test".into(),
        gpu_dedicated_memory_usage_kb: None,
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
    };
    let update = to_hardware_monitor_update(snap, &TemperatureUnit::Fahrenheit);
    assert_eq!(update.gpus[0].gpu_temperature, Some(212.0));
  }

  #[test]
  fn temperature_celsius_rounded() {
    let snap = MetricsSnapshot {
      cpu_usage: 0.0,
      memory_usage: 0.0,
      processors_usage: vec![],
      gpus: vec![GpuMetric {
        gpu_id: "x".into(),
        gpu_name: "x".into(),
        gpu_usage: None,
        gpu_temperature: Some(65.7),
        gpu_source: "Test".into(),
        gpu_dedicated_memory_usage_kb: None,
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
    };
    let update = to_hardware_monitor_update(snap, &TemperatureUnit::Celsius);
    assert_eq!(update.gpus[0].gpu_temperature, Some(66.0));
  }

  #[test]
  fn translation_passes_through_none_optionals() {
    let snap = MetricsSnapshot {
      cpu_usage: 0.0,
      memory_usage: 0.0,
      processors_usage: vec![],
      gpus: vec![GpuMetric {
        gpu_id: "x".into(),
        gpu_name: "x".into(),
        gpu_usage: None,
        gpu_temperature: None,
        gpu_source: "x".into(),
        gpu_dedicated_memory_usage_kb: None,
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
    };
    let update = to_hardware_monitor_update(snap, &TemperatureUnit::Fahrenheit);
    assert!(update.gpus[0].gpu_usage.is_none());
    assert!(update.gpus[0].gpu_temperature.is_none());
    assert!(update.gpus[0].gpu_dedicated_memory_usage_kb.is_none());
    assert!(update.gpus[0].gpu_cooler_level.is_none());
  }

  #[test]
  fn cpu_and_sensor_temperatures_converted_to_fahrenheit() {
    let snap = MetricsSnapshot {
      cpu_usage: 0.0,
      memory_usage: 0.0,
      processors_usage: vec![],
      gpus: vec![],
      processes: vec![],
      cpu_temperature: Some(50.0),
      cpu_package_thermal_status: None,
      sensor_temperatures: vec![
        SensorTemperature {
          name: "CPUZ".into(),
          temperature: 50.0,
        },
        SensorTemperature {
          name: "TZ01".into(),
          temperature: 40.0,
        },
      ],
      motherboard_temperatures: vec![],
      motherboard_fan_speeds: vec![],
      power_draw: Default::default(),
      cpu_power_support: Default::default(),
      motherboard_fan_support: Default::default(),
      external_component_guidance_candidates: vec![],
    };
    let update = to_hardware_monitor_update(snap, &TemperatureUnit::Fahrenheit);
    assert_eq!(update.cpu_temperature, Some(122.0));
    assert_eq!(update.sensor_temperatures.len(), 2);
    assert_eq!(update.sensor_temperatures[0].name, "CPUZ");
    assert_eq!(update.sensor_temperatures[0].value, 122);
    assert_eq!(update.sensor_temperatures[1].name, "TZ01");
    assert_eq!(update.sensor_temperatures[1].value, 104);
  }

  #[test]
  fn cpu_and_sensor_temperatures_passthrough_in_celsius() {
    let snap = MetricsSnapshot {
      cpu_usage: 0.0,
      memory_usage: 0.0,
      processors_usage: vec![],
      gpus: vec![],
      processes: vec![],
      cpu_temperature: Some(49.6),
      cpu_package_thermal_status: None,
      sensor_temperatures: vec![SensorTemperature {
        name: "TZ00".into(),
        temperature: 49.6,
      }],
      motherboard_temperatures: vec![],
      motherboard_fan_speeds: vec![],
      power_draw: Default::default(),
      cpu_power_support: Default::default(),
      motherboard_fan_support: Default::default(),
      external_component_guidance_candidates: vec![],
    };
    let update = to_hardware_monitor_update(snap, &TemperatureUnit::Celsius);
    assert_eq!(update.cpu_temperature, Some(50.0));
    assert_eq!(update.sensor_temperatures[0].value, 50);
  }

  #[test]
  fn motherboard_sensors_are_converted_with_source_and_fan_status() {
    let snap = MetricsSnapshot {
      cpu_usage: 0.0,
      memory_usage: 0.0,
      processors_usage: vec![],
      gpus: vec![],
      processes: vec![],
      cpu_temperature: None,
      cpu_package_thermal_status: None,
      sensor_temperatures: vec![],
      motherboard_temperatures: vec![MotherboardTemperature {
        name: "SYSTIN".into(),
        temperature: 40.0,
        source: "NCT6799D / Super I/O".into(),
      }],
      motherboard_fan_speeds: vec![MotherboardFanSpeed {
        name: "Fan 1".into(),
        rpm: Some(0),
        status: hardviz_core::models::FanSpeedStatus::Inactive,
        source: "NCT6799D / Super I/O".into(),
      }],
      power_draw: Default::default(),
      cpu_power_support: hardviz_core::models::SensorSupport::Supported,
      motherboard_fan_support: hardviz_core::models::SensorSupport::Supported,
      external_component_guidance_candidates: vec![],
    };

    let update = to_hardware_monitor_update(snap, &TemperatureUnit::Fahrenheit);

    assert_eq!(update.motherboard_temperatures.len(), 1);
    assert_eq!(update.motherboard_temperatures[0].name, "SYSTIN");
    assert_eq!(update.motherboard_temperatures[0].value, 104);
    assert_eq!(
      update.motherboard_temperatures[0].source,
      "NCT6799D / Super I/O"
    );
    assert_eq!(update.motherboard_fan_speeds.len(), 1);
    assert_eq!(
      update.cpu_power_support,
      crate::models::hardware::SensorSupport::Supported
    );
    assert_eq!(
      update.motherboard_fan_support,
      crate::models::hardware::SensorSupport::Supported
    );
    assert_eq!(update.motherboard_fan_speeds[0].rpm, Some(0));
    assert_eq!(
      update.motherboard_fan_speeds[0].status,
      crate::models::hardware::FanSpeedStatus::Inactive
    );
  }

  #[test]
  fn convert_temperature_celsius_passthrough() {
    assert_eq!(convert_temperature(25.0, &TemperatureUnit::Celsius), 25.0);
    assert_eq!(convert_temperature(25.4, &TemperatureUnit::Celsius), 25.0);
    assert_eq!(convert_temperature(25.5, &TemperatureUnit::Celsius), 26.0);
  }

  #[test]
  fn convert_temperature_fahrenheit() {
    assert_eq!(convert_temperature(0.0, &TemperatureUnit::Fahrenheit), 32.0);
    assert_eq!(
      convert_temperature(100.0, &TemperatureUnit::Fahrenheit),
      212.0
    );
    assert_eq!(
      convert_temperature(-40.0, &TemperatureUnit::Fahrenheit),
      -40.0
    );
  }

  #[test]
  fn main_window_visibility_gates_emission() {
    assert!(should_emit_for_main_window(Some(MainWindowState {
      is_visible: true,
      is_minimized: false,
    })));
    assert!(!should_emit_for_main_window(Some(MainWindowState {
      is_visible: false,
      is_minimized: false,
    })));
    assert!(!should_emit_for_main_window(Some(MainWindowState {
      is_visible: true,
      is_minimized: true,
    })));
    assert!(!should_emit_for_main_window(None));
  }

  #[test]
  fn latest_window_snapshot_keeps_the_most_recent_snapshot() {
    let latest = LatestWindowSnapshot::default();
    assert!(latest.load().is_none());

    latest.store(make_snapshot(1.0));
    latest.store(make_snapshot(2.0));

    assert_eq!(latest.load().unwrap().cpu_usage, 2.0);
  }

  #[test]
  fn window_snapshot_omits_process_samples() {
    let mut snapshot = make_snapshot(1.0);
    let candidate =
      hardviz_core::models::ExternalComponentGuidanceCandidate::pawnio_cpu_package_temperature(
        "PawnIOLib.dll not found".to_string(),
      );
    snapshot
      .processes
      .push(hardviz_core::models::ProcessSample {
        pid: 42,
        name: "test-process".into(),
        cpu_usage: 5.0,
        memory_kb: 1024.0,
        run_time_secs: 60,
      });
    snapshot
      .external_component_guidance_candidates
      .push(candidate.clone());

    let snapshot = to_window_snapshot(snapshot);

    assert!(snapshot.processes.is_empty());
    assert_eq!(
      snapshot.external_component_guidance_candidates,
      vec![candidate]
    );
  }
}
