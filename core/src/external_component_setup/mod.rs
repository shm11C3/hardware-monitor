//! External Component Setup: the explicit, user-initiated installation of an
//! optional external component from its pinned upstream release.
//!
//! Core owns the component catalog (pinned artifact URLs, sizes, digests,
//! module file names, installer switches), the verification rules, and the
//! OS-level setup steps behind the platform boundary. App owns the entry
//! points (command-line mode, IPC, installer custom actions) and the UI.
//!
//! The setup process reports back through its exit code only. A result file
//! or pipe in a user-writable location would let a same-user medium-integrity
//! process redirect an elevated write or forge the result; the exit code of a
//! process handle the caller owns cannot be forged.
//!
//! See `docs/adr/0024-external-component-setup.md` and
//! `docs/design/external-component-setup.md`.

use sha2::{Digest, Sha256};
use std::path::PathBuf;

use crate::models::ExternalComponent;

#[cfg(target_os = "windows")]
pub mod windows;

/// A release asset pinned by version, size, and SHA-256 digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinnedArtifact {
  pub version: &'static str,
  pub file_name: &'static str,
  pub url: &'static str,
  pub size: u64,
  pub sha256_hex: &'static str,
}

/// The runtime installer step of a setup plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallerStep {
  pub artifact: PinnedArtifact,
  /// Arguments for an unattended install. The installer elevates itself, so
  /// the process running it must already be elevated to avoid a second prompt.
  pub unattended_args: &'static [&'static str],
  /// Exit codes that mean "installed, restart Windows to finish".
  pub reboot_required_exit_codes: &'static [i32],
}

/// The module-file step of a setup plan: a zip archive whose listed entries
/// are placed under the component install location when missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileBundleStep {
  pub artifact: PinnedArtifact,
  pub file_names: &'static [&'static str],
}

/// Everything Core needs to set up one component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExternalComponentSetupPlan {
  pub component: ExternalComponent,
  pub installer: InstallerStep,
  pub file_bundle: FileBundleStep,
}

/// PawnIO runtime 2.2.0 from the PawnIO.Setup release. The digest was
/// computed from the downloaded asset on 2026-09-13 and matches the winget
/// `namazso.PawnIO` 2.2.0 manifest.
const PAWNIO_RUNTIME: PinnedArtifact = PinnedArtifact {
  version: "2.2.0",
  file_name: "PawnIO_setup.exe",
  url: "https://github.com/namazso/PawnIO.Setup/releases/download/2.2.0/PawnIO_setup.exe",
  size: 3_410_960,
  sha256_hex: "1f519a22e47187f70a1379a48ca604981c4fcf694f4e65b734aaa74a9fba3032",
};

/// PawnIO.Modules 0.2.8: the tag the sensor specification
/// (`docs/specs/sensors/pawnio-interface.md`) verified its IOCTL facts
/// against. Move this pin together with the specification, not ahead of it.
const PAWNIO_MODULES: PinnedArtifact = PinnedArtifact {
  version: "0.2.8",
  file_name: "release_0_2_8.zip",
  url: "https://github.com/namazso/PawnIO.Modules/releases/download/0.2.8/release_0_2_8.zip",
  size: 57_240,
  sha256_hex: "def304df8691cd2d2b700068bcbe8454ad97064e6621c71420c128d368d83fb7",
};

/// Signed module blobs the Windows providers can load. Names match the
/// provider's production file names; unsigned `.amx` fallbacks are not
/// installed because the production driver rejects them.
const PAWNIO_MODULE_FILES: &[&str] = &[
  "IntelMSR.bin",
  "RyzenSMU.bin",
  "AMDFamily17.bin",
  "LpcIO.bin",
];

/// `ERROR_SUCCESS_REBOOT_REQUIRED`: the PawnIO installer returns it in silent
/// mode when the driver install needs a restart (PawnIO.Setup 2.2.0 notes).
/// The setup process reuses it as its own exit code for the same meaning.
const ERROR_SUCCESS_REBOOT_REQUIRED: i32 = 3010;

const PAWNIO_SETUP_PLAN: ExternalComponentSetupPlan = ExternalComponentSetupPlan {
  component: ExternalComponent::Pawnio,
  installer: InstallerStep {
    artifact: PAWNIO_RUNTIME,
    unattended_args: &["-install", "-silent"],
    reboot_required_exit_codes: &[ERROR_SUCCESS_REBOOT_REQUIRED],
  },
  file_bundle: FileBundleStep {
    artifact: PAWNIO_MODULES,
    file_names: PAWNIO_MODULE_FILES,
  },
};

/// The setup plan for a component, or `None` when HardwareVisualizer does not
/// offer setup for it (for example `smartctl`, which is a system package).
pub fn setup_plan(
  component: ExternalComponent,
) -> Option<&'static ExternalComponentSetupPlan> {
  match component {
    ExternalComponent::Pawnio => Some(&PAWNIO_SETUP_PLAN),
    ExternalComponent::Smartctl => None,
  }
}

/// Components that have a setup plan, in display order.
pub fn components_with_setup() -> Vec<ExternalComponent> {
  [ExternalComponent::Pawnio, ExternalComponent::Smartctl]
    .into_iter()
    .filter(|component| setup_plan(*component).is_some())
    .collect()
}

/// Whether the component runtime is installed on this machine.
///
/// `Unknown` preserves enumeration uncertainty: a registry or file-system
/// failure is not evidence of absence and must not start an install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeInstallState {
  NotInstalled,
  Installed {
    version: Option<String>,
    install_location: Option<PathBuf>,
  },
  Unknown {
    detail: String,
  },
}

/// Presence of one module file the plan can place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleFileState {
  pub file_name: String,
  pub present: bool,
}

/// Why setup is not offered on this platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalComponentSetupSupport {
  Supported,
  UnsupportedPlatform,
}

/// The current state of one component as seen by the setup plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalComponentSetupStatus {
  pub component: ExternalComponent,
  pub support: ExternalComponentSetupSupport,
  pub runtime: RuntimeInstallState,
  pub module_files: Vec<ModuleFileState>,
  /// Set when module-file enumeration failed somewhere, so `module_files`
  /// may under-report presence. Setup refuses to run while this is set.
  pub enumeration_error: Option<String>,
  pub pinned_runtime_version: String,
  pub pinned_modules_version: String,
}

impl ExternalComponentSetupStatus {
  pub fn unsupported_platform(plan: &ExternalComponentSetupPlan) -> Self {
    Self {
      component: plan.component,
      support: ExternalComponentSetupSupport::UnsupportedPlatform,
      runtime: RuntimeInstallState::NotInstalled,
      module_files: plan
        .file_bundle
        .file_names
        .iter()
        .map(|file_name| ModuleFileState {
          file_name: (*file_name).to_string(),
          present: false,
        })
        .collect(),
      enumeration_error: None,
      pinned_runtime_version: plan.installer.artifact.version.to_string(),
      pinned_modules_version: plan.file_bundle.artifact.version.to_string(),
    }
  }

  /// True when the runtime is installed and every module file is present, so
  /// setup has nothing left to do. Uncertain enumeration is never complete.
  pub fn is_complete(&self) -> bool {
    matches!(self.runtime, RuntimeInstallState::Installed { .. })
      && self.enumeration_error.is_none()
      && self.module_files.iter().all(|file| file.present)
  }

  /// Why setup must not run right now, if the state is not trustworthy.
  pub fn setup_blocker(&self) -> Option<String> {
    if self.support != ExternalComponentSetupSupport::Supported {
      return Some("External Component Setup is available on Windows only".to_string());
    }
    if let RuntimeInstallState::Unknown { detail } = &self.runtime {
      return Some(format!("runtime state is unknown: {detail}"));
    }
    self
      .enumeration_error
      .as_ref()
      .map(|detail| format!("module file state is unknown: {detail}"))
  }

  pub fn missing_module_files(&self) -> Vec<&str> {
    self
      .module_files
      .iter()
      .filter(|file| !file.present)
      .map(|file| file.file_name.as_str())
      .collect()
  }
}

/// Where a setup run stopped. Carried in the setup process exit code so the
/// caller can explain the failure without a writable result channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupFailureStage {
  /// The current state could not be determined, so nothing was attempted.
  StateUnknown,
  /// The administrator-only staging directory could not be created.
  StagingDirectory,
  DownloadRuntime,
  VerifyRuntime,
  StartInstaller,
  InstallerExit,
  DownloadModules,
  VerifyModules,
  /// The verified archive does not contain a required module file.
  ArchiveContents,
  PlaceModules,
  /// Every step ran, but the component is still not complete.
  Incomplete,
  UnsupportedPlatform,
  /// A failure without a more specific stage, or an unrecognized exit code.
  Other,
}

impl SetupFailureStage {
  const ALL: [Self; 13] = [
    Self::StateUnknown,
    Self::StagingDirectory,
    Self::DownloadRuntime,
    Self::VerifyRuntime,
    Self::StartInstaller,
    Self::InstallerExit,
    Self::DownloadModules,
    Self::VerifyModules,
    Self::ArchiveContents,
    Self::PlaceModules,
    Self::Incomplete,
    Self::UnsupportedPlatform,
    Self::Other,
  ];

  /// Exit code of the setup process for this stage. `Other` is the generic
  /// failure code; the specific stages start at 10 so they never collide with
  /// conventional codes or `ERROR_SUCCESS_REBOOT_REQUIRED`.
  pub fn exit_code(self) -> i32 {
    match self {
      Self::Other => 1,
      Self::StateUnknown => 10,
      Self::StagingDirectory => 11,
      Self::DownloadRuntime => 12,
      Self::VerifyRuntime => 13,
      Self::StartInstaller => 14,
      Self::InstallerExit => 15,
      Self::DownloadModules => 16,
      Self::VerifyModules => 17,
      Self::ArchiveContents => 18,
      Self::PlaceModules => 19,
      Self::Incomplete => 20,
      Self::UnsupportedPlatform => 21,
    }
  }

  pub fn from_exit_code(code: i32) -> Option<Self> {
    Self::ALL
      .into_iter()
      .find(|stage| stage.exit_code() == code)
  }
}

/// The outcome of running a setup plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalComponentSetupOutcome {
  /// Nothing was missing, nothing was changed.
  AlreadyInstalled,
  /// The plan completed and the component is usable after the app restarts.
  Installed,
  /// The plan completed but Windows must restart before the driver is usable.
  RebootRequired,
  /// The plan stopped; the component may be partially set up.
  Failed {
    stage: SetupFailureStage,
    detail: String,
  },
}

impl ExternalComponentSetupOutcome {
  pub fn failed(stage: SetupFailureStage, detail: impl Into<String>) -> Self {
    Self::Failed {
      stage,
      detail: detail.into(),
    }
  }

  /// Process exit code of the setup command-line mode.
  pub fn exit_code(&self) -> i32 {
    match self {
      Self::AlreadyInstalled | Self::Installed => 0,
      Self::RebootRequired => ERROR_SUCCESS_REBOOT_REQUIRED,
      Self::Failed { stage, .. } => stage.exit_code(),
    }
  }

  /// Reconstruct the outcome the caller can trust from the exit code of the
  /// setup process it launched. `0` is reported as `Installed`; the caller
  /// short-circuits an already complete component before launching.
  pub fn from_exit_code(exit_code: Option<i32>) -> Self {
    match exit_code {
      Some(0) => Self::Installed,
      Some(ERROR_SUCCESS_REBOOT_REQUIRED) => Self::RebootRequired,
      Some(code) => Self::Failed {
        stage: SetupFailureStage::from_exit_code(code)
          .unwrap_or(SetupFailureStage::Other),
        detail: format!("the setup process exited with code {code}"),
      },
      None => Self::Failed {
        stage: SetupFailureStage::Other,
        detail: "the setup process exited without an exit code".to_string(),
      },
    }
  }
}

/// What a setup run did, as known inside the setup process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalComponentSetupResult {
  pub component: ExternalComponent,
  pub outcome: ExternalComponentSetupOutcome,
  pub runtime_installed: bool,
  pub module_files_placed: Vec<String>,
}

impl ExternalComponentSetupResult {
  pub fn failed(
    component: ExternalComponent,
    stage: SetupFailureStage,
    detail: impl Into<String>,
  ) -> Self {
    Self {
      component,
      outcome: ExternalComponentSetupOutcome::failed(stage, detail),
      runtime_installed: false,
      module_files_placed: Vec::new(),
    }
  }

  pub fn exit_code(&self) -> i32 {
    self.outcome.exit_code()
  }
}

/// Check a downloaded artifact against its pinned size and digest.
pub fn verify_artifact(bytes: &[u8], artifact: &PinnedArtifact) -> Result<(), String> {
  if bytes.len() as u64 != artifact.size {
    return Err(format!(
      "{} size mismatch: expected {} bytes, downloaded {} bytes",
      artifact.file_name,
      artifact.size,
      bytes.len()
    ));
  }

  let digest = Sha256::digest(bytes);
  let digest_hex = digest
    .iter()
    .map(|byte| format!("{byte:02x}"))
    .collect::<String>();
  if !digest_hex.eq_ignore_ascii_case(artifact.sha256_hex) {
    return Err(format!(
      "{} SHA-256 mismatch: expected {}, downloaded {}",
      artifact.file_name, artifact.sha256_hex, digest_hex
    ));
  }

  Ok(())
}

/// What an installer exit status means for the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallerExitOutcome {
  Installed,
  RebootRequired,
  Failed(Option<i32>),
}

pub fn interpret_installer_exit_code(
  step: &InstallerStep,
  exit_code: Option<i32>,
) -> InstallerExitOutcome {
  match exit_code {
    Some(0) => InstallerExitOutcome::Installed,
    Some(code) if step.reboot_required_exit_codes.contains(&code) => {
      InstallerExitOutcome::RebootRequired
    }
    other => InstallerExitOutcome::Failed(other),
  }
}

/// Map archive entry names to the missing module files they satisfy.
///
/// Matching uses the entry's base name, case-insensitively, so an archive
/// that nests files in a directory still resolves. Returns
/// `(entry_name, file_name)` pairs in the order of `missing`.
pub fn select_bundle_entries<'a>(
  entry_names: &'a [String],
  missing: &[&'a str],
) -> Vec<(&'a str, &'a str)> {
  missing
    .iter()
    .filter_map(|file_name| {
      entry_names
        .iter()
        .find(|entry| {
          entry
            .rsplit(['/', '\\'])
            .next()
            .is_some_and(|base| base.eq_ignore_ascii_case(file_name))
        })
        .map(|entry| (entry.as_str(), *file_name))
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn pawnio_has_a_setup_plan_and_smartctl_does_not() {
    assert!(setup_plan(ExternalComponent::Pawnio).is_some());
    assert!(setup_plan(ExternalComponent::Smartctl).is_none());
    assert_eq!(components_with_setup(), vec![ExternalComponent::Pawnio]);
  }

  #[test]
  fn pawnio_plan_pins_upstream_release_assets() {
    let plan = setup_plan(ExternalComponent::Pawnio).unwrap();

    assert_eq!(plan.installer.artifact.version, "2.2.0");
    assert!(
      plan
        .installer
        .artifact
        .url
        .starts_with("https://github.com/namazso/PawnIO.Setup/releases/download/2.2.0/")
    );
    assert_eq!(plan.installer.unattended_args, &["-install", "-silent"]);
    assert_eq!(plan.file_bundle.artifact.version, "0.2.8");
    assert_eq!(
      plan.file_bundle.file_names,
      &[
        "IntelMSR.bin",
        "RyzenSMU.bin",
        "AMDFamily17.bin",
        "LpcIO.bin"
      ]
    );
    assert_eq!(plan.installer.artifact.sha256_hex.len(), 64);
    assert_eq!(plan.file_bundle.artifact.sha256_hex.len(), 64);
  }

  #[test]
  fn verify_artifact_accepts_matching_size_and_digest() {
    let bytes = b"hello world";
    let artifact = PinnedArtifact {
      version: "1",
      file_name: "hello.txt",
      url: "https://example.invalid/hello.txt",
      size: 11,
      sha256_hex: "B94D27B9934D3E08A52E52D7DA7DABFAC484EFE37A5380EE9088F7ACE2EFCDE9",
    };

    assert_eq!(verify_artifact(bytes, &artifact), Ok(()));
  }

  #[test]
  fn verify_artifact_rejects_size_and_digest_mismatches() {
    let artifact = PinnedArtifact {
      version: "1",
      file_name: "hello.txt",
      url: "https://example.invalid/hello.txt",
      size: 11,
      sha256_hex: "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9",
    };

    let size_error = verify_artifact(b"hello", &artifact).unwrap_err();
    assert!(size_error.contains("size mismatch"), "{size_error}");

    let digest_error = verify_artifact(b"hello worle", &artifact).unwrap_err();
    assert!(digest_error.contains("SHA-256 mismatch"), "{digest_error}");
  }

  #[test]
  fn installer_exit_codes_map_to_plan_outcomes() {
    let step = setup_plan(ExternalComponent::Pawnio).unwrap().installer;

    assert_eq!(
      interpret_installer_exit_code(&step, Some(0)),
      InstallerExitOutcome::Installed
    );
    assert_eq!(
      interpret_installer_exit_code(&step, Some(3010)),
      InstallerExitOutcome::RebootRequired
    );
    assert_eq!(
      interpret_installer_exit_code(&step, Some(5)),
      InstallerExitOutcome::Failed(Some(5))
    );
    assert_eq!(
      interpret_installer_exit_code(&step, None),
      InstallerExitOutcome::Failed(None)
    );
  }

  #[test]
  fn select_bundle_entries_matches_base_names_case_insensitively() {
    let entries = vec![
      "COPYING".to_string(),
      "nested/intelmsr.bin".to_string(),
      "LpcIO.bin".to_string(),
    ];

    let selected =
      select_bundle_entries(&entries, &["IntelMSR.bin", "RyzenSMU.bin", "LpcIO.bin"]);

    assert_eq!(
      selected,
      vec![
        ("nested/intelmsr.bin", "IntelMSR.bin"),
        ("LpcIO.bin", "LpcIO.bin")
      ]
    );
  }

  #[test]
  fn status_reports_completeness_and_missing_files() {
    let plan = setup_plan(ExternalComponent::Pawnio).unwrap();
    let mut status = ExternalComponentSetupStatus::unsupported_platform(plan);
    assert!(!status.is_complete());
    assert_eq!(status.missing_module_files().len(), 4);
    assert!(status.setup_blocker().is_some());

    status.support = ExternalComponentSetupSupport::Supported;
    status.runtime = RuntimeInstallState::Installed {
      version: Some("2.2.0".to_string()),
      install_location: None,
    };
    for file in &mut status.module_files {
      file.present = true;
    }
    assert!(status.is_complete());
    assert!(status.missing_module_files().is_empty());
    assert_eq!(status.setup_blocker(), None);
  }

  #[test]
  fn uncertain_state_blocks_setup_and_is_never_complete() {
    let plan = setup_plan(ExternalComponent::Pawnio).unwrap();
    let mut status = ExternalComponentSetupStatus::unsupported_platform(plan);
    status.support = ExternalComponentSetupSupport::Supported;
    status.runtime = RuntimeInstallState::Installed {
      version: None,
      install_location: None,
    };
    for file in &mut status.module_files {
      file.present = true;
    }

    status.enumeration_error = Some("access denied".to_string());
    assert!(!status.is_complete());
    assert!(status.setup_blocker().unwrap().contains("access denied"));

    status.enumeration_error = None;
    status.runtime = RuntimeInstallState::Unknown {
      detail: "registry query failed".to_string(),
    };
    assert!(!status.is_complete());
    assert!(
      status
        .setup_blocker()
        .unwrap()
        .contains("registry query failed")
    );
  }

  #[test]
  fn failure_stages_round_trip_through_exit_codes() {
    for stage in SetupFailureStage::ALL {
      assert_eq!(
        SetupFailureStage::from_exit_code(stage.exit_code()),
        Some(stage)
      );
      assert_ne!(stage.exit_code(), 0);
      assert_ne!(stage.exit_code(), ERROR_SUCCESS_REBOOT_REQUIRED);
    }
    assert_eq!(SetupFailureStage::from_exit_code(99), None);
  }

  #[test]
  fn outcome_exit_codes_reconstruct_the_outcome() {
    let installed = ExternalComponentSetupOutcome::Installed;
    assert_eq!(
      ExternalComponentSetupOutcome::from_exit_code(Some(installed.exit_code())),
      installed
    );

    let reboot = ExternalComponentSetupOutcome::RebootRequired;
    assert_eq!(
      ExternalComponentSetupOutcome::from_exit_code(Some(reboot.exit_code())),
      reboot
    );

    let failed =
      ExternalComponentSetupOutcome::failed(SetupFailureStage::VerifyRuntime, "digest");
    match ExternalComponentSetupOutcome::from_exit_code(Some(failed.exit_code())) {
      ExternalComponentSetupOutcome::Failed { stage, detail } => {
        assert_eq!(stage, SetupFailureStage::VerifyRuntime);
        assert!(detail.contains("13"), "{detail}");
      }
      other => panic!("unexpected {other:?}"),
    }

    assert!(matches!(
      ExternalComponentSetupOutcome::from_exit_code(None),
      ExternalComponentSetupOutcome::Failed {
        stage: SetupFailureStage::Other,
        ..
      }
    ));
    assert!(matches!(
      ExternalComponentSetupOutcome::from_exit_code(Some(99)),
      ExternalComponentSetupOutcome::Failed {
        stage: SetupFailureStage::Other,
        ..
      }
    ));
  }

  #[test]
  fn result_exit_code_follows_the_outcome() {
    let result = ExternalComponentSetupResult::failed(
      ExternalComponent::Pawnio,
      SetupFailureStage::Incomplete,
      "x",
    );
    assert_eq!(result.exit_code(), 20);
  }
}
