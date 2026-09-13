use crate::enums::error::PlatformError;
use crate::models::hardware::{
  GpuMemoryUsage, GraphicInfo, MemoryInfo, NetworkInfo, SuperIoChipIdDiagnostics,
};
use crate::platform::traits::{
  ElevatedProcessRun, ExternalComponentSetupPlatform, GpuPlatform, GpuUsageRaw,
  MemoryPlatform, MotherboardPlatform, NetworkPlatform, Platform,
  ProcessElevationPlatform, SensorPlatform, SuperIoPlatform,
};
use async_trait::async_trait;
use tokio::task;

mod gpu;
pub mod memory;
pub mod motherboard;
pub mod network;

pub struct MacOSPlatform;

impl MacOSPlatform {
  pub fn new() -> Result<Self, PlatformError> {
    Ok(Self)
  }
}

#[async_trait]
impl MemoryPlatform for MacOSPlatform {
  async fn get_memory_info(&self) -> Result<MemoryInfo, PlatformError> {
    task::spawn_blocking(memory::get_memory_info)
      .await
      .map_err(|e| PlatformError::fault(format!("Failed to join memory task: {e}")))?
  }

  async fn get_memory_info_detail(&self) -> Result<MemoryInfo, PlatformError> {
    // macOS is not supported yet (build-only stub)
    Err(PlatformError::unsupported(
      "get_memory_info_detail is not implemented for MacOSPlatform",
    ))
  }
}

#[async_trait]
impl GpuPlatform for MacOSPlatform {
  async fn get_gpu_usage(&self) -> Result<GpuUsageRaw, PlatformError> {
    gpu::get_gpu_usage().await
  }

  async fn get_gpu_temperature(
    &self,
  ) -> Result<Vec<crate::models::hardware::NameValue>, PlatformError> {
    // macOS is not supported yet (build-only stub)
    Err(PlatformError::unsupported(
      "get_gpu_temperature is not implemented for MacOSPlatform",
    ))
  }

  async fn get_gpu_info(&self) -> Result<Vec<GraphicInfo>, PlatformError> {
    gpu::get_gpu_info().await
  }

  async fn get_gpu_memory_usage(&self) -> Result<Option<GpuMemoryUsage>, PlatformError> {
    gpu::get_gpu_memory_usage().await
  }

  async fn sample_gpus(&self) -> Vec<crate::models::GpuSample> {
    gpu::sample_gpus().await
  }

  fn sample_power_draw(&self) -> crate::models::PowerDraw {
    gpu::sample_power_draw()
  }

  fn cpu_power_support(&self) -> crate::models::SensorSupport {
    #[cfg(target_arch = "aarch64")]
    {
      crate::models::SensorSupport::Supported
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
      crate::models::SensorSupport::Unsupported
    }
  }
}

impl NetworkPlatform for MacOSPlatform {
  fn get_network_info(&self) -> Result<Vec<NetworkInfo>, PlatformError> {
    network::get_network_info()
  }
}

#[async_trait]
impl MotherboardPlatform for MacOSPlatform {
  async fn get_motherboard_info(
    &self,
  ) -> Result<crate::models::hardware::MotherboardInfo, PlatformError> {
    motherboard::get_motherboard_info().await
  }
}

impl SuperIoPlatform for MacOSPlatform {
  fn get_super_io_chip_id_diagnostics(&self) -> SuperIoChipIdDiagnostics {
    SuperIoChipIdDiagnostics::unsupported_platform()
  }
}

impl SensorPlatform for MacOSPlatform {
  fn sample_temperatures(&self) -> crate::models::TemperatureSample {
    crate::models::TemperatureSample::unsupported(
      "CPU and named sensor temperature sampling is not implemented for MacOSPlatform",
    )
  }

  fn sample_motherboard_sensors(&self) -> crate::models::MotherboardSensorCollection {
    crate::models::MotherboardSensorCollection::unsupported(
      "Motherboard sensor sampling is available on Windows only",
    )
  }
}

impl ProcessElevationPlatform for MacOSPlatform {
  fn is_process_elevated(&self) -> Result<bool, PlatformError> {
    Ok(false)
  }

  fn relaunch_current_process_elevated(&self) -> Result<(), PlatformError> {
    Err(PlatformError::unsupported(
      "Elevated Startup Mode is only supported on Windows.",
    ))
  }
}

impl ExternalComponentSetupPlatform for MacOSPlatform {
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
      "External Component Setup is available on Windows only.",
    )
  }

  fn run_current_executable_elevated(
    &self,
    _args: &[String],
  ) -> Result<ElevatedProcessRun, PlatformError> {
    Err(PlatformError::unsupported(
      "Elevated process launch is only supported on Windows.",
    ))
  }
}

impl Platform for MacOSPlatform {}

#[cfg(all(test, target_os = "macos"))]
mod tests {
  use super::*;

  #[tokio::test]
  async fn build_only_stubs_classify_as_unsupported() {
    let platform = MacOSPlatform::new().expect("MacOSPlatform::new never fails");

    assert!(matches!(
      platform.get_memory_info_detail().await,
      Err(PlatformError::Unsupported { .. })
    ));
    assert!(matches!(
      platform.get_gpu_temperature().await,
      Err(PlatformError::Unsupported { .. })
    ));
  }

  #[test]
  fn elevation_relaunch_classifies_as_unsupported() {
    let platform = MacOSPlatform::new().expect("MacOSPlatform::new never fails");

    assert!(matches!(
      platform.relaunch_current_process_elevated(),
      Err(PlatformError::Unsupported { .. })
    ));
    assert_eq!(platform.is_process_elevated(), Ok(false));
  }
}
