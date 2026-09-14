//! Command-line modes of the application binary that run without the Tauri
//! runtime.
//!
//! The only mode today is External Component Setup (ADR 0024): the Settings
//! action launches the executable elevated in this mode and the Windows
//! installer's custom action invokes it from its elevated context, so one
//! Core code path serves both entry points. The mode reports through its
//! exit code only; it never writes a result anywhere the caller could have
//! redirected.

use hardviz_core::external_component_setup::{
  ExternalComponentSetupOutcome, ExternalComponentSetupResult, SetupFailureStage,
  setup_plan,
};
use hardviz_core::models::ExternalComponent;
use hardviz_core::platform::factory::PlatformFactory;

pub const EXTERNAL_COMPONENT_SETUP_FLAG: &str = "--external-component-setup";

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
  ExternalComponentSetup { component: ExternalComponent },
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

  while let Some(arg) = args.next() {
    if arg == EXTERNAL_COMPONENT_SETUP_FLAG {
      let id = args
        .next()
        .ok_or(CliParseError::MissingValue(EXTERNAL_COMPONENT_SETUP_FLAG))?;
      component =
        Some(component_from_cli_id(&id).ok_or(CliParseError::UnknownComponent(id))?);
    }
  }

  Ok(component.map(|component| CliMode::ExternalComponentSetup { component }))
}

/// Run a command-line mode to completion and return the process exit code.
pub fn run_cli_mode(mode: CliMode) -> i32 {
  match mode {
    CliMode::ExternalComponentSetup { component } => {
      let result = run_external_component_setup(component);
      if let ExternalComponentSetupOutcome::Failed { stage, detail } = &result.outcome {
        eprintln!("external component setup failed at {stage:?}: {detail}");
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
      SetupFailureStage::Other,
      "this component has no External Component Setup plan",
    );
  };
  match PlatformFactory::shared() {
    Ok(platform) => platform.run_external_component_setup(plan),
    Err(e) => ExternalComponentSetupResult::failed(
      component,
      SetupFailureStage::Other,
      e.to_string(),
    ),
  }
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
  fn parses_external_component_setup() {
    let mode = parse_cli_mode([
      "hardware-visualizer.exe",
      "--external-component-setup",
      "pawnio",
    ])
    .unwrap();

    assert_eq!(
      mode,
      Some(CliMode::ExternalComponentSetup {
        component: ExternalComponent::Pawnio,
      })
    );
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
  fn a_component_without_a_plan_exits_with_the_generic_failure_code() {
    assert_eq!(
      run_cli_mode(CliMode::ExternalComponentSetup {
        component: ExternalComponent::Smartctl,
      }),
      SetupFailureStage::Other.exit_code()
    );
  }
}
