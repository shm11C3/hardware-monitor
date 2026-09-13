use crate::enums::error::PlatformError;
use crate::models;
use async_trait::async_trait;

/// Return type for [`GpuPlatform::get_gpu_usage`]: `(percentage, source_name)`.
pub type GpuUsageRaw = (f32, String);

/// Trait that defines platform-specific memory operations
#[async_trait]
pub trait MemoryPlatform: Send + Sync {
  /// Get basic memory information
  async fn get_memory_info(&self) -> Result<models::hardware::MemoryInfo, PlatformError>;

  /// Get detailed memory information (supported platforms only)
  async fn get_memory_info_detail(
    &self,
  ) -> Result<models::hardware::MemoryInfo, PlatformError>;
}

/// Trait that defines platform-specific GPU operations
#[async_trait]
pub trait GpuPlatform: Send + Sync {
  /// Get GPU usage together with the data-source name
  async fn get_gpu_usage(&self) -> Result<GpuUsageRaw, PlatformError>;

  /// Get GPU temperatures, always in raw degrees Celsius.
  ///
  /// Presentation conversion (Celsius/Fahrenheit) is the App's
  /// responsibility — Core never reads the user's preferred unit so the
  /// trait stays decoupled from UI preferences.
  async fn get_gpu_temperature(
    &self,
  ) -> Result<Vec<models::hardware::NameValue>, PlatformError>;

  /// Get GPU information
  async fn get_gpu_info(
    &self,
  ) -> Result<Vec<models::hardware::GraphicInfo>, PlatformError>;

  /// Get realtime GPU memory usage (best-effort)
  async fn get_gpu_memory_usage(
    &self,
  ) -> Result<Option<models::hardware::GpuMemoryUsage>, PlatformError>;

  /// Collect per-GPU realtime metrics for the monitoring pipeline.
  async fn sample_gpus(&self) -> Vec<models::GpuSample>;

  /// Read the latest platform-wide live power sample.
  fn sample_power_draw(&self) -> models::PowerDraw {
    models::PowerDraw::default()
  }

  /// Report whether this hardware has a supported CPU package-power path.
  fn cpu_power_support(&self) -> models::SensorSupport {
    models::SensorSupport::Unsupported
  }
}

/// Trait that defines platform-specific network operations
pub trait NetworkPlatform: Send + Sync {
  /// Get network information
  #[allow(dead_code)]
  fn get_network_info(
    &self,
  ) -> Result<Vec<crate::models::hardware::NetworkInfo>, PlatformError>;
}

/// Trait that defines platform-specific motherboard operations
#[async_trait]
pub trait MotherboardPlatform: Send + Sync {
  /// Get motherboard and BIOS information
  async fn get_motherboard_info(
    &self,
  ) -> Result<models::hardware::MotherboardInfo, PlatformError>;
}

/// Trait that defines platform-specific Super I/O chip-id diagnostics.
pub trait SuperIoPlatform: Send + Sync {
  /// Read raw Super I/O chip-id diagnostics.
  ///
  /// This is a blocking, read-only probe. Platforms without a Super I/O
  /// LpcIO path return a result with `platform_supported = false` rather
  /// than erroring, so the caller can branch on support uniformly.
  fn get_super_io_chip_id_diagnostics(
    &self,
  ) -> models::hardware::SuperIoChipIdDiagnostics;
}

/// Trait that defines live CPU / motherboard sensor sampling operations.
pub trait SensorPlatform: Send + Sync {
  /// Sample CPU and named temperature sensors, always in raw degrees Celsius.
  fn sample_temperatures(&self) -> models::TemperatureSample;

  /// Sample live motherboard temperature and fan readings.
  fn sample_motherboard_sensors(&self) -> models::MotherboardSensorCollection;
}

/// Trait that defines process elevation operations.
pub trait ProcessElevationPlatform: Send + Sync {
  /// Returns whether the current process is running with elevated privileges.
  fn is_process_elevated(&self) -> Result<bool, PlatformError>;

  /// Relaunch the current executable with elevated privileges.
  fn relaunch_current_process_elevated(&self) -> Result<(), PlatformError>;
}

/// Result of launching the current executable elevated and waiting for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElevatedProcessRun {
  /// The user declined the elevation prompt; nothing ran.
  Declined,
  /// The elevated process ran to completion.
  Exited { exit_code: Option<i32> },
}

/// Trait that defines platform-specific External Component Setup operations.
pub trait ExternalComponentSetupPlatform: Send + Sync {
  /// Report the installed state of the component the plan describes.
  fn external_component_setup_status(
    &self,
    plan: &crate::external_component_setup::ExternalComponentSetupPlan,
  ) -> crate::external_component_setup::ExternalComponentSetupStatus;

  /// Run the plan in the current process. The caller must already be
  /// elevated where the platform requires it.
  fn run_external_component_setup(
    &self,
    plan: &crate::external_component_setup::ExternalComponentSetupPlan,
  ) -> crate::external_component_setup::ExternalComponentSetupResult;

  /// Launch the current executable elevated with `args` and wait for exit.
  fn run_current_executable_elevated(
    &self,
    args: &[String],
  ) -> Result<ElevatedProcessRun, PlatformError>;
}

/// Trait that integrates all platform functionality
pub trait Platform:
  MemoryPlatform
  + GpuPlatform
  + NetworkPlatform
  + MotherboardPlatform
  + SuperIoPlatform
  + SensorPlatform
  + ProcessElevationPlatform
  + ExternalComponentSetupPlatform
{
}
