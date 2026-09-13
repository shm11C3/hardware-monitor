//! App-side orchestration of External Component Setup (ADR 0023).
//!
//! The service never installs anything in the app process. It reads the
//! component state through Core, and for a setup run it launches the current
//! executable elevated in its setup command-line mode, waits for it, and
//! reads the JSON result the elevated process wrote.

use std::path::{Path, PathBuf};

use hardviz_core::external_component_setup::{
  self as core_setup, ExternalComponentSetupPlan, setup_plan,
};
use hardviz_core::models::ExternalComponent;
use hardviz_core::platform::factory::PlatformFactory;
use hardviz_core::platform::traits::ElevatedProcessRun;

use crate::cli::{EXTERNAL_COMPONENT_SETUP_FLAG, RESULT_FILE_FLAG, component_cli_id};
use crate::log_warn;
use crate::models::external_component_setup::ExternalComponentSetupResult;

fn plan_for(
  component: ExternalComponent,
) -> Result<&'static ExternalComponentSetupPlan, String> {
  setup_plan(component).ok_or_else(|| {
    format!(
      "{} has no External Component Setup plan",
      component_cli_id(component)
    )
  })
}

pub fn status(
  component: ExternalComponent,
) -> Result<core_setup::ExternalComponentSetupStatus, String> {
  let plan = plan_for(component)?;
  let platform = PlatformFactory::shared().map_err(|e| e.to_string())?;
  Ok(platform.external_component_setup_status(plan))
}

/// Run setup for `component` in an elevated child process and report the
/// outcome together with the refreshed state. Blocks until the child exits;
/// call it from a blocking task.
pub fn run(component: ExternalComponent) -> Result<ExternalComponentSetupResult, String> {
  let plan = plan_for(component)?;
  let platform = PlatformFactory::shared().map_err(|e| e.to_string())?;

  let before = platform.external_component_setup_status(plan);
  if before.support != core_setup::ExternalComponentSetupSupport::Supported {
    return Ok(ExternalComponentSetupResult::failed(
      before,
      "External Component Setup is not available on this platform",
    ));
  }
  if before.is_complete() {
    return Ok(ExternalComponentSetupResult::from_core(
      core_setup::ExternalComponentSetupResult {
        component,
        outcome: core_setup::ExternalComponentSetupOutcome::AlreadyInstalled,
        runtime_installed: false,
        module_files_placed: Vec::new(),
      },
      before,
    ));
  }

  let result_file = result_file_path(component);
  let _ = std::fs::remove_file(&result_file);
  let args = setup_args(component, &result_file);

  let run = platform
    .run_current_executable_elevated(&args)
    .map_err(|e| e.to_string())?;
  let after = platform.external_component_setup_status(plan);

  let result = match run {
    ElevatedProcessRun::Declined => ExternalComponentSetupResult::cancelled(after),
    ElevatedProcessRun::Exited { exit_code } => {
      match read_result_file(&result_file, component) {
        Ok(core_result) => ExternalComponentSetupResult::from_core(core_result, after),
        Err(detail) => {
          log_warn!(
            format!("setup result unavailable: {detail}"),
            "external_component_setup_service::run",
            None::<&str>
          );
          ExternalComponentSetupResult::failed(
            after,
            format!(
              "the setup process exited with {} but left no readable result: {detail}",
              exit_code
                .map_or_else(|| "no exit code".to_string(), |code| code.to_string())
            ),
          )
        }
      }
    }
  };
  let _ = std::fs::remove_file(&result_file);

  Ok(result)
}

fn setup_args(component: ExternalComponent, result_file: &Path) -> Vec<String> {
  vec![
    EXTERNAL_COMPONENT_SETUP_FLAG.to_string(),
    component_cli_id(component).to_string(),
    RESULT_FILE_FLAG.to_string(),
    result_file.display().to_string(),
  ]
}

fn result_file_path(component: ExternalComponent) -> PathBuf {
  std::env::temp_dir().join(format!(
    "hardviz-external-component-setup-{}-{}.json",
    component_cli_id(component),
    std::process::id()
  ))
}

fn read_result_file(
  path: &Path,
  component: ExternalComponent,
) -> Result<core_setup::ExternalComponentSetupResult, String> {
  let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
  let result: core_setup::ExternalComponentSetupResult =
    serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
  if result.component != component {
    return Err(format!(
      "{} describes {} instead of {}",
      path.display(),
      component_cli_id(result.component),
      component_cli_id(component)
    ));
  }
  Ok(result)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn setup_args_name_the_component_and_result_file() {
    let args = setup_args(ExternalComponent::Pawnio, Path::new(r"C:\Temp\r.json"));

    assert_eq!(
      args,
      vec![
        "--external-component-setup",
        "pawnio",
        "--result-file",
        r"C:\Temp\r.json"
      ]
    );
  }

  #[test]
  fn read_result_file_rejects_a_result_for_another_component() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("result.json");
    let result = core_setup::ExternalComponentSetupResult::failed(
      ExternalComponent::Smartctl,
      "nope",
    );
    std::fs::write(&path, serde_json::to_vec(&result).unwrap()).unwrap();

    let error = read_result_file(&path, ExternalComponent::Pawnio).unwrap_err();

    assert!(error.contains("smartctl"), "{error}");
  }

  #[test]
  fn read_result_file_reports_a_missing_file() {
    let error = read_result_file(
      Path::new("/definitely/missing/result.json"),
      ExternalComponent::Pawnio,
    )
    .unwrap_err();

    assert!(error.contains("result.json"), "{error}");
  }

  #[cfg(not(target_os = "windows"))]
  #[test]
  fn run_reports_unsupported_platform_without_launching_anything() {
    let result = run(ExternalComponent::Pawnio).unwrap();

    assert_eq!(
      result.outcome,
      crate::models::external_component_setup::ExternalComponentSetupOutcome::Failed
    );
    assert_eq!(
      result.status.support,
      crate::models::external_component_setup::ExternalComponentSetupSupport::UnsupportedPlatform
    );
  }

  #[test]
  fn smartctl_has_no_plan() {
    assert!(status(ExternalComponent::Smartctl).is_err());
  }
}
