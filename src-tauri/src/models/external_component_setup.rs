use hardviz_core::external_component_setup as core_setup;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::external_component_guidance::ExternalComponent;

// No doc comments on enum variants in this file: tauri-specta renders them
// as a multi-line union with trailing whitespace, which the CI whitespace
// gate rejects.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ExternalComponentSetupSupport {
  Supported,
  UnsupportedPlatform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ExternalComponentRuntimeInstallState {
  NotInstalled,
  Installed,
  Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExternalComponentRuntimeState {
  pub state: ExternalComponentRuntimeInstallState,
  pub version: Option<String>,
  pub install_location: Option<String>,
  /// Why the state is unknown, when it is.
  pub detail: Option<String>,
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
  /// Why setup cannot run right now (unsupported platform or an uncertain
  /// state), or `None` when it can.
  pub setup_blocker: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ExternalComponentSetupOutcome {
  AlreadyInstalled,
  Installed,
  RebootRequired,
  Cancelled,
  Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ExternalComponentSetupFailureStage {
  StateUnknown,
  StagingDirectory,
  DownloadRuntime,
  VerifyRuntime,
  StartInstaller,
  InstallerExit,
  DownloadModules,
  VerifyModules,
  ArchiveContents,
  PlaceModules,
  Incomplete,
  UnsupportedPlatform,
  Panicked,
  Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExternalComponentSetupResult {
  pub component: ExternalComponent,
  pub outcome: ExternalComponentSetupOutcome,
  /// Present for `failed` outcomes.
  pub failure_stage: Option<ExternalComponentSetupFailureStage>,
  /// Free-text detail known to the app process (never taken from the
  /// elevated child, which reports through its exit code only).
  pub detail: Option<String>,
  /// The state after the run, so the UI does not need a second call.
  pub status: ExternalComponentSetupStatus,
}

impl From<core_setup::ExternalComponentSetupStatus> for ExternalComponentSetupStatus {
  fn from(src: core_setup::ExternalComponentSetupStatus) -> Self {
    let complete = src.is_complete();
    let setup_blocker = src.setup_blocker();
    let runtime = match src.runtime {
      core_setup::RuntimeInstallState::NotInstalled => ExternalComponentRuntimeState {
        state: ExternalComponentRuntimeInstallState::NotInstalled,
        version: None,
        install_location: None,
        detail: None,
      },
      core_setup::RuntimeInstallState::Installed {
        version,
        install_location,
      } => ExternalComponentRuntimeState {
        state: ExternalComponentRuntimeInstallState::Installed,
        version,
        install_location: install_location.map(|path| path.display().to_string()),
        detail: None,
      },
      core_setup::RuntimeInstallState::Unknown { detail } => {
        ExternalComponentRuntimeState {
          state: ExternalComponentRuntimeInstallState::Unknown,
          version: None,
          install_location: None,
          detail: Some(detail),
        }
      }
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
      setup_blocker,
    }
  }
}

impl From<core_setup::SetupFailureStage> for ExternalComponentSetupFailureStage {
  fn from(value: core_setup::SetupFailureStage) -> Self {
    match value {
      core_setup::SetupFailureStage::StateUnknown => Self::StateUnknown,
      core_setup::SetupFailureStage::StagingDirectory => Self::StagingDirectory,
      core_setup::SetupFailureStage::DownloadRuntime => Self::DownloadRuntime,
      core_setup::SetupFailureStage::VerifyRuntime => Self::VerifyRuntime,
      core_setup::SetupFailureStage::StartInstaller => Self::StartInstaller,
      core_setup::SetupFailureStage::InstallerExit => Self::InstallerExit,
      core_setup::SetupFailureStage::DownloadModules => Self::DownloadModules,
      core_setup::SetupFailureStage::VerifyModules => Self::VerifyModules,
      core_setup::SetupFailureStage::ArchiveContents => Self::ArchiveContents,
      core_setup::SetupFailureStage::PlaceModules => Self::PlaceModules,
      core_setup::SetupFailureStage::Incomplete => Self::Incomplete,
      core_setup::SetupFailureStage::UnsupportedPlatform => Self::UnsupportedPlatform,
      core_setup::SetupFailureStage::Panicked => Self::Panicked,
      core_setup::SetupFailureStage::Other => Self::Other,
    }
  }
}

impl ExternalComponentSetupResult {
  pub fn from_outcome(
    outcome: core_setup::ExternalComponentSetupOutcome,
    status: core_setup::ExternalComponentSetupStatus,
  ) -> Self {
    let (outcome, failure_stage, detail) = match outcome {
      core_setup::ExternalComponentSetupOutcome::AlreadyInstalled => {
        (ExternalComponentSetupOutcome::AlreadyInstalled, None, None)
      }
      core_setup::ExternalComponentSetupOutcome::Installed => {
        (ExternalComponentSetupOutcome::Installed, None, None)
      }
      core_setup::ExternalComponentSetupOutcome::RebootRequired => {
        (ExternalComponentSetupOutcome::RebootRequired, None, None)
      }
      core_setup::ExternalComponentSetupOutcome::Failed { stage, detail } => (
        ExternalComponentSetupOutcome::Failed,
        Some(stage.into()),
        Some(detail),
      ),
    };

    Self {
      component: status.component.into(),
      outcome,
      failure_stage,
      detail,
      status: status.into(),
    }
  }

  pub fn cancelled(status: core_setup::ExternalComponentSetupStatus) -> Self {
    Self {
      component: status.component.into(),
      outcome: ExternalComponentSetupOutcome::Cancelled,
      failure_stage: None,
      detail: None,
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
    assert_eq!(
      wire.runtime.state,
      ExternalComponentRuntimeInstallState::NotInstalled
    );
    assert_eq!(wire.module_files.len(), 4);
    assert!(!wire.complete);
    assert!(wire.setup_blocker.is_some());
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

    assert_eq!(
      wire.runtime.state,
      ExternalComponentRuntimeInstallState::Installed
    );
    assert_eq!(wire.runtime.version.as_deref(), Some("2.2.0"));
    assert_eq!(
      wire.runtime.install_location.as_deref(),
      Some(r"C:\Program Files\PawnIO")
    );
    assert!(wire.complete);
    assert_eq!(wire.setup_blocker, None);
  }

  #[test]
  fn unknown_runtime_state_carries_detail_and_blocks_setup() {
    let mut status =
      core_setup::ExternalComponentSetupStatus::unsupported_platform(plan());
    status.support = core_setup::ExternalComponentSetupSupport::Supported;
    status.runtime = core_setup::RuntimeInstallState::Unknown {
      detail: "registry unavailable".to_string(),
    };

    let wire: ExternalComponentSetupStatus = status.into();

    assert_eq!(
      wire.runtime.state,
      ExternalComponentRuntimeInstallState::Unknown
    );
    assert_eq!(wire.runtime.detail.as_deref(), Some("registry unavailable"));
    assert!(wire.setup_blocker.unwrap().contains("registry unavailable"));
  }

  #[test]
  fn maps_failed_outcome_with_stage_and_detail() {
    let status = core_setup::ExternalComponentSetupStatus::unsupported_platform(plan());

    let wire = ExternalComponentSetupResult::from_outcome(
      core_setup::ExternalComponentSetupOutcome::failed(
        core_setup::SetupFailureStage::VerifyRuntime,
        "digest mismatch",
      ),
      status,
    );

    assert_eq!(wire.outcome, ExternalComponentSetupOutcome::Failed);
    assert_eq!(
      wire.failure_stage,
      Some(ExternalComponentSetupFailureStage::VerifyRuntime)
    );
    assert_eq!(wire.detail.as_deref(), Some("digest mismatch"));
  }

  #[test]
  fn cancelled_result_carries_no_detail() {
    let status = core_setup::ExternalComponentSetupStatus::unsupported_platform(plan());

    let wire = ExternalComponentSetupResult::cancelled(status);

    assert_eq!(wire.outcome, ExternalComponentSetupOutcome::Cancelled);
    assert_eq!(wire.failure_stage, None);
    assert_eq!(wire.detail, None);
  }
}
