use crate::enums::error::PlatformError;
use crate::models::hardware::{
  GpuMemoryUsage, GraphicInfo, MemoryInfo, MotherboardInfo, NetworkInfo,
  SuperIoChipIdDiagnostics,
};
use crate::platform::traits::{
  ElevatedProcessRun, ExternalComponentSetupPlatform, GpuPlatform, GpuUsageRaw,
  MemoryPlatform, MotherboardPlatform, NetworkPlatform, Platform,
  ProcessElevationPlatform, SensorPlatform, SuperIoPlatform,
};
use async_trait::async_trait;

pub mod gpu;
pub mod memory;
pub mod motherboard;
pub mod network;
pub mod process_elevation;
pub mod sensors;

pub struct WindowsPlatform;

impl WindowsPlatform {
  pub fn new() -> Result<Self, PlatformError> {
    Ok(Self)
  }
}

#[async_trait]
impl MemoryPlatform for WindowsPlatform {
  async fn get_memory_info(&self) -> Result<MemoryInfo, PlatformError> {
    memory::get_memory_info().await
  }

  async fn get_memory_info_detail(&self) -> Result<MemoryInfo, PlatformError> {
    memory::get_memory_info_detail().await
  }
}

#[async_trait]
impl GpuPlatform for WindowsPlatform {
  async fn get_gpu_usage(&self) -> Result<GpuUsageRaw, PlatformError> {
    gpu::get_gpu_usage().await
  }

  async fn get_gpu_temperature(
    &self,
  ) -> Result<Vec<crate::models::hardware::NameValue>, PlatformError> {
    gpu::get_gpu_temperature().await
  }

  async fn get_gpu_info(&self) -> Result<Vec<GraphicInfo>, PlatformError> {
    gpu::get_gpu_info().await
  }

  async fn get_gpu_memory_usage(&self) -> Result<Option<GpuMemoryUsage>, PlatformError> {
    Ok(None)
  }

  async fn sample_gpus(&self) -> Vec<crate::models::GpuSample> {
    gpu::sample_gpus().await
  }

  fn sample_power_draw(&self) -> crate::models::PowerDraw {
    sensors::sample_power_draw()
  }

  fn cpu_power_support(&self) -> crate::models::SensorSupport {
    sensors::cpu_power_support()
  }
}

impl NetworkPlatform for WindowsPlatform {
  fn get_network_info(&self) -> Result<Vec<NetworkInfo>, PlatformError> {
    network::get_network_info()
  }
}

#[async_trait]
impl MotherboardPlatform for WindowsPlatform {
  async fn get_motherboard_info(&self) -> Result<MotherboardInfo, PlatformError> {
    motherboard::get_motherboard_info().await
  }
}

impl SuperIoPlatform for WindowsPlatform {
  fn get_super_io_chip_id_diagnostics(&self) -> SuperIoChipIdDiagnostics {
    crate::infrastructure::providers::windows::super_io_diagnostics::read_super_io_chip_id_diagnostics()
  }
}

impl SensorPlatform for WindowsPlatform {
  fn sample_temperatures(&self) -> crate::models::TemperatureSample {
    sensors::sample_temperatures()
  }

  fn sample_motherboard_sensors(&self) -> crate::models::MotherboardSensorCollection {
    sensors::sample_motherboard_sensors()
  }
}

impl ProcessElevationPlatform for WindowsPlatform {
  fn is_process_elevated(&self) -> Result<bool, PlatformError> {
    process_elevation::is_process_elevated()
  }

  fn relaunch_current_process_elevated(&self) -> Result<(), PlatformError> {
    process_elevation::relaunch_current_process_elevated()
  }
}

impl ExternalComponentSetupPlatform for WindowsPlatform {
  fn external_component_setup_status(
    &self,
    plan: &crate::external_component_setup::ExternalComponentSetupPlan,
  ) -> crate::external_component_setup::ExternalComponentSetupStatus {
    crate::external_component_setup::windows::status(plan)
  }

  fn run_external_component_setup(
    &self,
    plan: &crate::external_component_setup::ExternalComponentSetupPlan,
  ) -> crate::external_component_setup::ExternalComponentSetupResult {
    crate::external_component_setup::windows::run(plan)
  }

  fn run_current_executable_elevated(
    &self,
    args: &[String],
  ) -> Result<ElevatedProcessRun, PlatformError> {
    process_elevation::run_current_executable_elevated(args)
  }
}

impl Platform for WindowsPlatform {}
