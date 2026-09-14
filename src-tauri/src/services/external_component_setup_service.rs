//! App-side orchestration of External Component Setup (ADR 0024).
//!
//! The service never installs anything in the app process. It reads the
//! component state through Core, and for a setup run it launches the current
//! executable elevated in its setup command-line mode, waits for it, and
//! derives the outcome from the exit code of the process handle it owns. No
//! result file or pipe exists for a same-user process to redirect or forge.

use std::sync::Mutex;

use hardviz_core::external_component_setup::{
  self as core_setup, ExternalComponentSetupPlan, setup_plan,
};
use hardviz_core::models::ExternalComponent;
use hardviz_core::platform::factory::PlatformFactory;
use hardviz_core::platform::traits::ElevatedProcessRun;

use crate::cli::{EXTERNAL_COMPONENT_SETUP_FLAG, component_cli_id};
use crate::log_warn;
use crate::models::external_component_setup::ExternalComponentSetupResult;

/// Components with a setup run in flight. One elevated setup per component at
/// a time; a second request while one runs is rejected instead of starting a
/// duplicate installer.
static IN_FLIGHT: Mutex<Vec<ExternalComponent>> = Mutex::new(Vec::new());

struct InFlightGuard(ExternalComponent);

impl InFlightGuard {
  fn acquire(component: ExternalComponent) -> Result<Self, String> {
    let mut in_flight = IN_FLIGHT
      .lock()
      .unwrap_or_else(|poisoned| poisoned.into_inner());
    if in_flight.contains(&component) {
      return Err(format!(
        "External Component Setup for {} is already running",
        component_cli_id(component)
      ));
    }
    in_flight.push(component);
    Ok(Self(component))
  }
}

impl Drop for InFlightGuard {
  fn drop(&mut self) {
    let mut in_flight = IN_FLIGHT
      .lock()
      .unwrap_or_else(|poisoned| poisoned.into_inner());
    in_flight.retain(|component| *component != self.0);
  }
}

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
  let _guard = InFlightGuard::acquire(component)?;

  let before = platform.external_component_setup_status(plan);
  if let Some(blocker) = before.setup_blocker() {
    let stage = if before.support == core_setup::ExternalComponentSetupSupport::Supported
    {
      core_setup::SetupFailureStage::StateUnknown
    } else {
      core_setup::SetupFailureStage::UnsupportedPlatform
    };
    return Ok(ExternalComponentSetupResult::from_outcome(
      core_setup::ExternalComponentSetupOutcome::failed(stage, blocker),
      before,
    ));
  }
  if before.is_complete() {
    return Ok(ExternalComponentSetupResult::from_outcome(
      core_setup::ExternalComponentSetupOutcome::AlreadyInstalled,
      before,
    ));
  }

  let args = setup_args(component);
  let run = platform
    .run_current_executable_elevated(&args)
    .map_err(|e| e.to_string())?;
  let after = platform.external_component_setup_status(plan);

  Ok(match run {
    ElevatedProcessRun::Declined => ExternalComponentSetupResult::cancelled(after),
    ElevatedProcessRun::Exited { exit_code } => {
      let outcome = core_setup::ExternalComponentSetupOutcome::from_exit_code(exit_code);
      if let core_setup::ExternalComponentSetupOutcome::Failed { stage, detail } =
        &outcome
      {
        log_warn!(
          format!(
            "external component setup for {} failed at {stage:?}: {detail}",
            component_cli_id(component)
          ),
          "external_component_setup_service::run",
          None::<&str>
        );
      }
      ExternalComponentSetupResult::from_outcome(outcome, after)
    }
  })
}

fn setup_args(component: ExternalComponent) -> Vec<String> {
  vec![
    EXTERNAL_COMPONENT_SETUP_FLAG.to_string(),
    component_cli_id(component).to_string(),
  ]
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn setup_args_name_the_component_only() {
    assert_eq!(
      setup_args(ExternalComponent::Pawnio),
      vec!["--external-component-setup", "pawnio"]
    );
  }

  #[test]
  fn in_flight_guard_rejects_a_duplicate_and_releases_on_drop() {
    let first = InFlightGuard::acquire(ExternalComponent::Smartctl).unwrap();
    assert!(InFlightGuard::acquire(ExternalComponent::Smartctl).is_err());
    let other = InFlightGuard::acquire(ExternalComponent::Pawnio).unwrap();
    drop(first);
    assert!(InFlightGuard::acquire(ExternalComponent::Smartctl).is_ok());
    drop(other);
  }

  #[cfg(not(target_os = "windows"))]
  #[test]
  fn run_reports_unsupported_platform_without_launching_anything() {
    use crate::models::external_component_setup::{
      ExternalComponentSetupFailureStage, ExternalComponentSetupOutcome,
      ExternalComponentSetupSupport,
    };

    let result = run(ExternalComponent::Pawnio).unwrap();

    assert_eq!(result.outcome, ExternalComponentSetupOutcome::Failed);
    assert_eq!(
      result.failure_stage,
      Some(ExternalComponentSetupFailureStage::UnsupportedPlatform)
    );
    assert_eq!(
      result.status.support,
      ExternalComponentSetupSupport::UnsupportedPlatform
    );
  }

  #[test]
  fn smartctl_has_no_plan() {
    assert!(status(ExternalComponent::Smartctl).is_err());
  }
}
