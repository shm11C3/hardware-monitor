//! Command-line modes of the application binary that run without the Tauri
//! runtime.
//!
//! The only mode today is External Component Setup (ADR 0024): the Settings
//! action launches the executable elevated in this mode and the Windows
//! installer's custom action invokes it from its elevated context, so one
//! Core code path serves both entry points.

use std::path::PathBuf;

use hardviz_core::external_component_setup::{ExternalComponentSetupResult, setup_plan};
use hardviz_core::models::ExternalComponent;
use hardviz_core::platform::factory::PlatformFactory;

pub const EXTERNAL_COMPONENT_SETUP_FLAG: &str = "--external-component-setup";
pub const RESULT_FILE_FLAG: &str = "--result-file";

/// Stable command-line identifiers for components with a setup plan.
pub fn component_cli_id(component: ExternalComponent) -> &'static str {
  match component {
    ExternalComponent::Pawnio => "pawnio",
    ExternalComponent::Smartctl => "smartctl",
  }
}

fn component_from_cli_id(id: &str) -> Option<ExternalComponent> {
  match id {
    "pawnio" => Some(ExternalComponent::Pawnio),
    "smartctl" => Some(ExternalComponent::Smartctl),
    _ => None,
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliMode {
  ExternalComponentSetup {
    component: ExternalComponent,
    result_file: Option<PathBuf>,
  },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliParseError {
  UnknownComponent(String),
  MissingValue(&'static str),
}

/// Recognize a command-line mode. Returns `Ok(None)` for a normal app launch.
pub fn parse_cli_mode<I, S>(args: I) -> Result<Option<CliMode>, CliParseError>
where
  I: IntoIterator<Item = S>,
  S: AsRef<str>,
{
  let mut args = args.into_iter().map(|arg| arg.as_ref().to_string());
  let mut component = None;
  let mut result_file = None;

  while let Some(arg) = args.next() {
    match arg.as_str() {
      EXTERNAL_COMPONENT_SETUP_FLAG => {
        let id = args
          .next()
          .ok_or(CliParseError::MissingValue(EXTERNAL_COMPONENT_SETUP_FLAG))?;
        component =
          Some(component_from_cli_id(&id).ok_or(CliParseError::UnknownComponent(id))?);
      }
      RESULT_FILE_FLAG => {
        result_file = Some(PathBuf::from(
          args
            .next()
            .ok_or(CliParseError::MissingValue(RESULT_FILE_FLAG))?,
        ));
      }
      _ => {}
    }
  }

  Ok(component.map(|component| CliMode::ExternalComponentSetup {
    component,
    result_file,
  }))
}

/// Run a command-line mode to completion and return the process exit code.
pub fn run_cli_mode(mode: CliMode) -> i32 {
  match mode {
    CliMode::ExternalComponentSetup {
      component,
      result_file,
    } => {
      let result = run_external_component_setup(component);
      if let Some(path) = result_file
        && let Err(e) = write_result_file(&path, &result)
      {
        eprintln!("failed to write {}: {e}", path.display());
      }
      result.exit_code()
    }
  }
}

fn run_external_component_setup(
  component: ExternalComponent,
) -> ExternalComponentSetupResult {
  let Some(plan) = setup_plan(component) else {
    return ExternalComponentSetupResult::failed(
      component,
      "this component has no External Component Setup plan",
    );
  };
  match PlatformFactory::shared() {
    Ok(platform) => platform.run_external_component_setup(plan),
    Err(e) => ExternalComponentSetupResult::failed(component, e.to_string()),
  }
}

fn write_result_file(
  path: &std::path::Path,
  result: &ExternalComponentSetupResult,
) -> std::io::Result<()> {
  let json = serde_json::to_vec_pretty(result).map_err(std::io::Error::other)?;
  std::fs::write(path, json)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn plain_launch_is_not_a_cli_mode() {
    assert_eq!(parse_cli_mode(["hardware-visualizer.exe"]), Ok(None));
    assert_eq!(
      parse_cli_mode(["hardware-visualizer.exe", "--some-tauri-flag"]),
      Ok(None)
    );
  }

  #[test]
  fn parses_external_component_setup_with_result_file() {
    let mode = parse_cli_mode([
      "hardware-visualizer.exe",
      "--external-component-setup",
      "pawnio",
      "--result-file",
      r"C:\Temp\result.json",
    ])
    .unwrap();

    assert_eq!(
      mode,
      Some(CliMode::ExternalComponentSetup {
        component: ExternalComponent::Pawnio,
        result_file: Some(PathBuf::from(r"C:\Temp\result.json")),
      })
    );
  }

  #[test]
  fn result_file_is_optional_for_installer_callers() {
    let mode = parse_cli_mode(["exe", "--external-component-setup", "pawnio"]).unwrap();

    assert!(matches!(
      mode,
      Some(CliMode::ExternalComponentSetup {
        component: ExternalComponent::Pawnio,
        result_file: None,
      })
    ));
  }

  #[test]
  fn rejects_unknown_components_and_missing_values() {
    assert_eq!(
      parse_cli_mode(["exe", "--external-component-setup", "winring0"]),
      Err(CliParseError::UnknownComponent("winring0".to_string()))
    );
    assert_eq!(
      parse_cli_mode(["exe", "--external-component-setup"]),
      Err(CliParseError::MissingValue(EXTERNAL_COMPONENT_SETUP_FLAG))
    );
    assert_eq!(
      parse_cli_mode([
        "exe",
        "--external-component-setup",
        "pawnio",
        "--result-file"
      ]),
      Err(CliParseError::MissingValue(RESULT_FILE_FLAG))
    );
  }

  #[test]
  fn cli_ids_round_trip() {
    for component in [ExternalComponent::Pawnio, ExternalComponent::Smartctl] {
      assert_eq!(
        component_from_cli_id(component_cli_id(component)),
        Some(component)
      );
    }
  }

  #[test]
  fn writes_the_result_file_as_json() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("result.json");
    let result = ExternalComponentSetupResult::failed(ExternalComponent::Pawnio, "nope");

    write_result_file(&path, &result).unwrap();

    let parsed: ExternalComponentSetupResult =
      serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(parsed, result);
  }
}
