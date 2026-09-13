use super::ExternalComponentGuidanceCandidate;

/// One GPU sample collected per physical GPU.
///
/// `None` means the metric is unavailable for this GPU vendor / platform.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuSample {
  pub gpu_id: String,
  pub name: String,
  pub usage: Option<f32>,
  pub temperature: Option<f32>,
  pub dedicated_memory_kb: Option<f32>,
  pub cooler_level: Option<u32>,
  pub source: String,
}

/// Live package-level Intel thermal and power-limitation status.
///
/// This value is present only when the package status register was read
/// successfully. The fields are current-state bits; sticky/log history bits
/// are intentionally not represented here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpuPackageThermalStatus {
  /// Intel package Thermal Status (`0`).
  pub thermal_status: bool,
  /// Intel package PROCHOT# or FORCEPR# event status (`2`).
  pub prochot_or_forcepr_asserted: bool,
  /// Intel package Power Limitation Status (`10`).
  pub power_limitation_status: bool,
}

/// Live platform power readings in watts.
///
/// `package_watts`, when available, is the derived CPU + GPU + ANE total;
/// a CPU package-domain reading belongs in `cpu_watts`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PowerDraw {
  pub cpu_watts: Option<f32>,
  pub gpu_watts: Option<f32>,
  pub ane_watts: Option<f32>,
  pub package_watts: Option<f32>,
}

/// Whether the current hardware has a supported path for a sensor family.
///
/// This is deliberately separate from a reading: supported hardware may not
/// have produced a value yet, while an absent historical value does not prove
/// that the hardware is unsupported.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SensorSupport {
  #[default]
  Unknown,
  Supported,
  Unsupported,
}

/// One sample of per-GPU metrics, normalized across vendors and platforms.
///
/// `None` indicates the metric is unavailable for this vendor/platform.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuMetric {
  pub gpu_id: String,
  pub gpu_name: String,
  pub gpu_usage: Option<f32>,
  pub gpu_temperature: Option<f32>,
  pub gpu_source: String,
  pub gpu_dedicated_memory_usage_kb: Option<f32>,
  pub gpu_cooler_level: Option<u32>,
}

/// Availability state for best-effort live sensor sampling.
///
/// Unsupported means the platform or hardware path is not available by design.
/// Unavailable means the platform supports the path, but the current sample
/// could not produce user-visible values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum SensorAvailability {
  #[default]
  Available,
  Unsupported {
    reason: String,
  },
  Unavailable {
    reason: String,
  },
}

impl SensorAvailability {
  pub fn unsupported(reason: impl Into<String>) -> Self {
    Self::Unsupported {
      reason: reason.into(),
    }
  }

  pub fn unavailable(reason: impl Into<String>) -> Self {
    Self::Unavailable {
      reason: reason.into(),
    }
  }
}

/// Runtime enablement decision for a native sensor path.
///
/// Prefer `Experimental` when an existing read-only access path recognizes the
/// hardware and an existing plausibility-gated decode can be attempted safely.
/// Use `Unsupported` when attempting the path would require guessing hardware
/// facts such as an address, register map, or chip selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorEnablement {
  Verified,
  Experimental,
  Unsupported,
}

/// One named temperature sensor reading (e.g. an ACPI thermal zone),
/// always in raw °C.
///
/// `name` is a short zone label such as `TZ00` or `CPUZ`. Presentation
/// conversion (Celsius/Fahrenheit) is the App's responsibility — Core
/// never reads the user's preferred unit.
#[derive(Debug, Clone, PartialEq)]
pub struct SensorTemperature {
  pub name: String,
  pub temperature: f32,
}

/// One named motherboard temperature reading, always in raw degrees C.
#[derive(Debug, Clone, PartialEq)]
pub struct MotherboardTemperature {
  pub name: String,
  pub temperature: f32,
  pub source: String,
}

/// Display classification for one motherboard fan-speed reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanSpeedStatus {
  Active,
  Inactive,
  Invalid,
}

/// One named motherboard fan-speed reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MotherboardFanSpeed {
  pub name: String,
  pub rpm: Option<u32>,
  pub status: FanSpeedStatus,
  pub source: String,
}

/// One round of motherboard sensor readings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MotherboardSensorSample {
  pub temperatures: Vec<MotherboardTemperature>,
  pub fan_speeds: Vec<MotherboardFanSpeed>,
}

/// One round of CPU / sensor temperature readings, always in raw °C.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TemperatureSample {
  pub cpu_temperature: Option<f32>,
  pub sensor_temperatures: Vec<SensorTemperature>,
  pub cpu_package_thermal_status: Option<CpuPackageThermalStatus>,
  pub availability: SensorAvailability,
  pub guidance_candidates: Vec<ExternalComponentGuidanceCandidate>,
}

impl TemperatureSample {
  pub fn unsupported(reason: impl Into<String>) -> Self {
    Self {
      availability: SensorAvailability::unsupported(reason),
      ..Self::default()
    }
  }

  pub fn unavailable(reason: impl Into<String>) -> Self {
    Self {
      availability: SensorAvailability::unavailable(reason),
      ..Self::default()
    }
  }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MotherboardSensorCollection {
  pub sample: MotherboardSensorSample,
  pub availability: SensorAvailability,
  pub fan_support: SensorSupport,
  pub guidance_candidates: Vec<ExternalComponentGuidanceCandidate>,
}

impl MotherboardSensorCollection {
  pub fn unsupported(reason: impl Into<String>) -> Self {
    Self {
      availability: SensorAvailability::unsupported(reason),
      fan_support: SensorSupport::Unsupported,
      ..Self::default()
    }
  }

  pub fn unavailable(reason: impl Into<String>) -> Self {
    Self {
      availability: SensorAvailability::unavailable(reason),
      fan_support: SensorSupport::Unknown,
      ..Self::default()
    }
  }
}

/// One sample of per-process metrics carried inside [`MetricsSnapshot`].
///
/// Persistence subscribers consume these to build per-process rolling
/// stats without sharing the collector's `Arc<Mutex<...>>` history maps.
/// The window adapter ignores them.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessSample {
  pub pid: u32,
  pub name: String,
  /// Raw per-tick CPU usage as reported by sysinfo (sum across all cores).
  pub cpu_usage: f32,
  /// Memory usage in KB, matching the existing on-disk archive format.
  pub memory_kb: f32,
  /// Process run time in seconds.
  pub run_time_secs: u64,
}

/// One real-time snapshot of system + GPU metrics, fanned out via
/// [`crate::event_bus::EventBus`] to all in-process subscribers
/// (window adapter, persistence, future tray/overlay/alert consumers).
#[derive(Debug, Clone, PartialEq)]
pub struct MetricsSnapshot {
  pub cpu_usage: f32,
  pub memory_usage: f32,
  pub processors_usage: Vec<f32>,
  pub gpus: Vec<GpuMetric>,
  /// Live platform power readings in watts. All fields are `None` when
  /// unsupported, not sampled yet, or invalid for the current tick.
  pub power_draw: PowerDraw,
  /// Whether CPU package-power collection is supported by this hardware.
  pub cpu_power_support: SensorSupport,
  pub processes: Vec<ProcessSample>,
  /// Headline CPU temperature in raw °C. `None` when no readable sensor
  /// exists on this platform (currently collected on Windows only).
  pub cpu_temperature: Option<f32>,
  /// Live Intel package thermal and power-limitation status. `None` when
  /// the package status register is unavailable or the platform is not Intel.
  pub cpu_package_thermal_status: Option<CpuPackageThermalStatus>,
  /// All named temperature sensors in raw °C (ACPI thermal zones on
  /// Windows). Empty when unsupported.
  pub sensor_temperatures: Vec<SensorTemperature>,
  /// Live motherboard temperature readings in raw degrees C. Empty when
  /// unsupported or unavailable.
  pub motherboard_temperatures: Vec<MotherboardTemperature>,
  /// Live motherboard fan-speed readings. Empty when unsupported or unavailable.
  pub motherboard_fan_speeds: Vec<MotherboardFanSpeed>,
  /// Whether motherboard fan-speed collection is supported by this hardware.
  pub motherboard_fan_support: SensorSupport,
  /// Diagnostic side data for optional runtime components that were
  /// attempted but unavailable while user-visible data remains missing.
  pub external_component_guidance_candidates: Vec<ExternalComponentGuidanceCandidate>,
}
