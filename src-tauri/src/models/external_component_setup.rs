use hardviz_core::external_component_setup as core_setup;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::external_component_guidance::ExternalComponent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ExternalComponentSetupSupport {
  Supported,
  UnsupportedPlatform,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExternalComponentRuntimeState {
  pub installed: bool,
  pub version: Option<String>,
  pub install_location: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExternalComponentModuleFileState {
  pub file_name: String,
  pub present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExternalComponentSetupStatus {
  pub component: ExternalComponent,
  pub support: ExternalComponentSetupSupport,
  pub runtime: ExternalComponentRuntimeState,
  pub module_files: Vec<ExternalComponentModuleFileState>,
  pub pinned_runtime_version: String,
  pub pinned_modules_version: String,
  /// True when nothing is left for setup to do.
  pub complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ExternalComponentSetupOutcome {
  AlreadyInstalled,
  Installed,
  RebootRequired,
  // No doc comments on variants: tauri-specta renders them as a multi-line
  // union with trailing whitespace, which fails the whitespace gate.
  // Cancelled: the user declined the elevation prompt; nothing ran.
  Cancelled,
  Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExternalComponentSetupResult {
  pub component: ExternalComponent,
  pub outcome: ExternalComponentSetupOutcome,
  pub detail: Option<String>,
  pub runtime_installed: bool,
  pub module_files_placed: Vec<String>,
  /// The state after the run, so the UI does not need a second call.
  pub status: ExternalComponentSetupStatus,
}

impl From<core_setup::ExternalComponentSetupStatus> for ExternalComponentSetupStatus {
  fn from(src: core_setup::ExternalComponentSetupStatus) -> Self {
    let complete = src.is_complete();
    let runtime = match src.runtime {
      core_setup::RuntimeInstallState::NotInstalled => ExternalComponentRuntimeState {
        installed: false,
        version: None,
        install_location: None,
      },
      core_setup::RuntimeInstallState::Installed {
        version,
        install_location,
      } => ExternalComponentRuntimeState {
        installed: true,
        version,
        install_location: install_location.map(|path| path.display().to_string()),
      },
    };

    Self {
      component: src.component.into(),
      support: match src.support {
        core_setup::ExternalComponentSetupSupport::Supported => {
          ExternalComponentSetupSupport::Supported
        }
        core_setup::ExternalComponentSetupSupport::UnsupportedPlatform => {
          ExternalComponentSetupSupport::UnsupportedPlatform
        }
      },
      runtime,
      module_files: src
        .module_files
        .into_iter()
        .map(|file| ExternalComponentModuleFileState {
          file_name: file.file_name,
          present: file.present,
        })
        .collect(),
      pinned_runtime_version: src.pinned_runtime_version,
      pinned_modules_version: src.pinned_modules_version,
      complete,
    }
  }
}

impl ExternalComponentSetupResult {
  pub fn from_core(
    src: core_setup::ExternalComponentSetupResult,
    status: core_setup::ExternalComponentSetupStatus,
  ) -> Self {
    let (outcome, detail) = match src.outcome {
      core_setup::ExternalComponentSetupOutcome::AlreadyInstalled => {
        (ExternalComponentSetupOutcome::AlreadyInstalled, None)
      }
      core_setup::ExternalComponentSetupOutcome::Installed => {
        (ExternalComponentSetupOutcome::Installed, None)
      }
      core_setup::ExternalComponentSetupOutcome::RebootRequired => {
        (ExternalComponentSetupOutcome::RebootRequired, None)
      }
      core_setup::ExternalComponentSetupOutcome::Failed { detail } => {
        (ExternalComponentSetupOutcome::Failed, Some(detail))
      }
    };

    Self {
      component: src.component.into(),
      outcome,
      detail,
      runtime_installed: src.runtime_installed,
      module_files_placed: src.module_files_placed,
      status: status.into(),
    }
  }

  pub fn cancelled(status: core_setup::ExternalComponentSetupStatus) -> Self {
    Self {
      component: status.component.into(),
      outcome: ExternalComponentSetupOutcome::Cancelled,
      detail: None,
      runtime_installed: false,
      module_files_placed: Vec::new(),
      status: status.into(),
    }
  }

  pub fn failed(
    status: core_setup::ExternalComponentSetupStatus,
    detail: impl Into<String>,
  ) -> Self {
    Self {
      component: status.component.into(),
      outcome: ExternalComponentSetupOutcome::Failed,
      detail: Some(detail.into()),
      runtime_installed: false,
      module_files_placed: Vec::new(),
      status: status.into(),
    }
  }
}

impl From<ExternalComponent> for hardviz_core::models::ExternalComponent {
  fn from(value: ExternalComponent) -> Self {
    match value {
      ExternalComponent::Pawnio => Self::Pawnio,
      ExternalComponent::Smartctl => Self::Smartctl,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use hardviz_core::models::ExternalComponent as CoreComponent;

  fn plan() -> &'static core_setup::ExternalComponentSetupPlan {
    core_setup::setup_plan(CoreComponent::Pawnio).unwrap()
  }

  #[test]
  fn converts_unsupported_platform_status_to_wire_shape() {
    let wire: ExternalComponentSetupStatus =
      core_setup::ExternalComponentSetupStatus::unsupported_platform(plan()).into();

    assert_eq!(wire.component, ExternalComponent::Pawnio);
    assert_eq!(
      wire.support,
      ExternalComponentSetupSupport::UnsupportedPlatform
    );
    assert!(!wire.runtime.installed);
    assert_eq!(wire.module_files.len(), 4);
    assert!(!wire.complete);
    assert_eq!(wire.pinned_runtime_version, "2.2.0");
  }

  #[test]
  fn converts_installed_status_and_marks_completeness() {
    let mut status =
      core_setup::ExternalComponentSetupStatus::unsupported_platform(plan());
    status.support = core_setup::ExternalComponentSetupSupport::Supported;
    status.runtime = core_setup::RuntimeInstallState::Installed {
      version: Some("2.2.0".to_string()),
      install_location: Some(std::path::PathBuf::from(r"C:\Program Files\PawnIO")),
    };
    for file in &mut status.module_files {
      file.present = true;
    }

    let wire: ExternalComponentSetupStatus = status.into();

    assert!(wire.runtime.installed);
    assert_eq!(wire.runtime.version.as_deref(), Some("2.2.0"));
    assert_eq!(
      wire.runtime.install_location.as_deref(),
      Some(r"C:\Program Files\PawnIO")
    );
    assert!(wire.complete);
  }

  #[test]
  fn maps_failed_core_result_with_detail() {
    let status = core_setup::ExternalComponentSetupStatus::unsupported_platform(plan());
    let core_result = core_setup::ExternalComponentSetupResult::failed(
      CoreComponent::Pawnio,
      "download failed",
    );

    let wire = ExternalComponentSetupResult::from_core(core_result, status);

    assert_eq!(wire.outcome, ExternalComponentSetupOutcome::Failed);
    assert_eq!(wire.detail.as_deref(), Some("download failed"));
  }

  #[test]
  fn cancelled_result_carries_no_detail() {
    let status = core_setup::ExternalComponentSetupStatus::unsupported_platform(plan());

    let wire = ExternalComponentSetupResult::cancelled(status);

    assert_eq!(wire.outcome, ExternalComponentSetupOutcome::Cancelled);
    assert_eq!(wire.detail, None);
  }
}
