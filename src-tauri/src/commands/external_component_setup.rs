use tauri::command;

use crate::models::external_component_guidance::ExternalComponent;
use crate::models::external_component_setup::{
  ExternalComponentSetupResult, ExternalComponentSetupStatus,
};
use crate::services::external_component_setup_service;

/// Components that HardwareVisualizer can set up on the user's request, in
/// display order.
#[command]
#[specta::specta]
pub fn get_external_component_setup_components() -> Vec<ExternalComponent> {
  hardviz_core::external_component_setup::components_with_setup()
    .into_iter()
    .map(Into::into)
    .collect()
}

#[command]
#[specta::specta]
pub async fn get_external_component_setup_status(
  component: ExternalComponent,
) -> Result<ExternalComponentSetupStatus, String> {
  let component: hardviz_core::models::ExternalComponent = component.into();
  tauri::async_runtime::spawn_blocking(move || {
    external_component_setup_service::status(component)
  })
  .await
  .map_err(|e| format!("external component setup status task failed: {e}"))?
  .map(Into::into)
}

/// Run External Component Setup in an elevated child process and wait for it.
#[command]
#[specta::specta]
pub async fn run_external_component_setup(
  component: ExternalComponent,
) -> Result<ExternalComponentSetupResult, String> {
  let component: hardviz_core::models::ExternalComponent = component.into();
  tauri::async_runtime::spawn_blocking(move || {
    external_component_setup_service::run(component)
  })
  .await
  .map_err(|e| format!("external component setup task failed: {e}"))?
}
