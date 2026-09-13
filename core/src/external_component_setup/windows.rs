//! Windows execution of an External Component Setup plan.
//!
//! This module runs inside an already elevated process: the App launches the
//! executable elevated in its setup command-line mode, or the installer's
//! custom action calls it from its elevated context. It downloads the pinned
//! artifacts, verifies them, runs the runtime installer unattended, and places
//! only the module files that are missing. It never overwrites or removes
//! anything.

use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::{
  ExternalComponentSetupOutcome, ExternalComponentSetupPlan,
  ExternalComponentSetupResult, ExternalComponentSetupStatus,
  ExternalComponentSetupSupport, FileBundleStep, InstallerExitOutcome, ModuleFileState,
  PinnedArtifact, RuntimeInstallState, interpret_installer_exit_code,
  parse_reg_query_output, select_bundle_entries, verify_artifact,
};
use crate::models::ExternalComponent;
use crate::{log_info, log_warn};

/// Uninstall registry key the PawnIO installer registers; `InstallLocation`
/// is the documented discovery path (`docs/specs/sensors/pawnio-interface.md`).
const PAWNIO_UNINSTALL_KEY: &str =
  r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\PawnIO";
const PAWNIO_DIRECTORY_NAME: &str = "PawnIO";
/// Matches the provider's recursive module search depth so "present" here
/// agrees with what collection would find.
const MODULE_SEARCH_DEPTH: usize = 4;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);

pub fn status(plan: &ExternalComponentSetupPlan) -> ExternalComponentSetupStatus {
  let runtime = runtime_install_state(plan.component);
  let roots = install_roots(&runtime);
  let module_files = plan
    .file_bundle
    .file_names
    .iter()
    .map(|file_name| ModuleFileState {
      file_name: (*file_name).to_string(),
      present: roots
        .iter()
        .any(|root| find_named_file(root, file_name, MODULE_SEARCH_DEPTH).is_some()),
    })
    .collect();

  ExternalComponentSetupStatus {
    component: plan.component,
    support: ExternalComponentSetupSupport::Supported,
    runtime,
    module_files,
    pinned_runtime_version: plan.installer.artifact.version.to_string(),
    pinned_modules_version: plan.file_bundle.artifact.version.to_string(),
  }
}

pub fn run(plan: &ExternalComponentSetupPlan) -> ExternalComponentSetupResult {
  let mut result = ExternalComponentSetupResult {
    component: plan.component,
    outcome: ExternalComponentSetupOutcome::AlreadyInstalled,
    runtime_installed: false,
    module_files_placed: Vec::new(),
  };

  let before = status(plan);
  if before.is_complete() {
    return result;
  }

  let temp_dir = match tempfile::Builder::new()
    .prefix("hardviz-external-component-setup-")
    .tempdir()
  {
    Ok(dir) => dir,
    Err(e) => {
      result.outcome = ExternalComponentSetupOutcome::Failed {
        detail: format!("failed to create a temporary directory: {e}"),
      };
      return result;
    }
  };

  let mut reboot_required = false;
  if matches!(before.runtime, RuntimeInstallState::NotInstalled) {
    match install_runtime(plan, temp_dir.path()) {
      Ok(InstallerExitOutcome::Installed) => result.runtime_installed = true,
      Ok(InstallerExitOutcome::RebootRequired) => {
        result.runtime_installed = true;
        reboot_required = true;
      }
      Ok(InstallerExitOutcome::Failed(code)) => {
        result.outcome = ExternalComponentSetupOutcome::Failed {
          detail: format!(
            "{} exited with {}",
            plan.installer.artifact.file_name,
            code.map_or_else(|| "no exit code".to_string(), |code| code.to_string())
          ),
        };
        return result;
      }
      Err(detail) => {
        result.outcome = ExternalComponentSetupOutcome::Failed { detail };
        return result;
      }
    }
  }

  let after = status(plan);
  let missing = after.missing_module_files();
  if !missing.is_empty() {
    let destination = module_destination(&after.runtime);
    match place_module_files(&plan.file_bundle, &missing, &destination) {
      Ok(placed) => result.module_files_placed = placed,
      Err(detail) => {
        result.outcome = ExternalComponentSetupOutcome::Failed { detail };
        return result;
      }
    }
  }

  result.outcome = if reboot_required {
    ExternalComponentSetupOutcome::RebootRequired
  } else {
    ExternalComponentSetupOutcome::Installed
  };
  result
}

fn runtime_install_state(component: ExternalComponent) -> RuntimeInstallState {
  let key = match component {
    ExternalComponent::Pawnio => PAWNIO_UNINSTALL_KEY,
    ExternalComponent::Smartctl => return RuntimeInstallState::NotInstalled,
  };

  let output = match Command::new("reg.exe").args(["query", key]).output() {
    Ok(output) if output.status.success() => output,
    _ => return RuntimeInstallState::NotInstalled,
  };
  let values = parse_reg_query_output(&String::from_utf8_lossy(&output.stdout));

  RuntimeInstallState::Installed {
    version: values.get("DisplayVersion").cloned(),
    install_location: values
      .get("InstallLocation")
      .filter(|value| !value.is_empty())
      .map(PathBuf::from),
  }
}

/// Every directory where module files count as present, mirroring the
/// provider's candidate order.
fn install_roots(runtime: &RuntimeInstallState) -> Vec<PathBuf> {
  let mut roots = Vec::new();
  if let RuntimeInstallState::Installed {
    install_location: Some(location),
    ..
  } = runtime
  {
    roots.push(location.clone());
  }
  for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
    if let Some(value) = std::env::var_os(var) {
      let path = PathBuf::from(value).join(PAWNIO_DIRECTORY_NAME);
      if !roots.iter().any(|root| root == &path) {
        roots.push(path);
      }
    }
  }
  roots
}

/// Where new module files go: the registered install location, otherwise the
/// documented `%ProgramFiles%\PawnIO` fallback.
fn module_destination(runtime: &RuntimeInstallState) -> PathBuf {
  if let RuntimeInstallState::Installed {
    install_location: Some(location),
    ..
  } = runtime
  {
    return location.clone();
  }

  std::env::var_os("ProgramFiles")
    .map(PathBuf::from)
    .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"))
    .join(PAWNIO_DIRECTORY_NAME)
}

fn install_runtime(
  plan: &ExternalComponentSetupPlan,
  temp_dir: &Path,
) -> Result<InstallerExitOutcome, String> {
  let artifact = &plan.installer.artifact;
  let bytes = download_verified(artifact)?;
  let installer_path = temp_dir.join(artifact.file_name);
  fs::write(&installer_path, &bytes)
    .map_err(|e| format!("failed to write {}: {e}", installer_path.display()))?;

  log_info!(
    format!(
      "running {} {}",
      artifact.file_name,
      plan.installer.unattended_args.join(" ")
    ),
    "external_component_setup::install_runtime",
    None::<&str>
  );
  let status = Command::new(&installer_path)
    .args(plan.installer.unattended_args)
    .status()
    .map_err(|e| format!("failed to start {}: {e}", artifact.file_name))?;

  Ok(interpret_installer_exit_code(
    &plan.installer,
    status.code(),
  ))
}

fn place_module_files(
  bundle: &FileBundleStep,
  missing: &[&str],
  destination: &Path,
) -> Result<Vec<String>, String> {
  let bytes = download_verified(&bundle.artifact)?;
  let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
    .map_err(|e| format!("failed to open {}: {e}", bundle.artifact.file_name))?;
  let entry_names = archive.file_names().map(str::to_string).collect::<Vec<_>>();
  let selected = select_bundle_entries(&entry_names, missing);
  if selected.len() != missing.len() {
    let found = selected
      .iter()
      .map(|(_, file_name)| *file_name)
      .collect::<Vec<_>>();
    let absent = missing
      .iter()
      .filter(|file_name| !found.contains(file_name))
      .copied()
      .collect::<Vec<_>>();
    return Err(format!(
      "{} does not contain {}",
      bundle.artifact.file_name,
      absent.join(", ")
    ));
  }

  fs::create_dir_all(destination)
    .map_err(|e| format!("failed to create {}: {e}", destination.display()))?;

  let mut placed = Vec::new();
  for (entry_name, file_name) in selected {
    let target = destination.join(file_name);
    if target.exists() {
      // Another setup or the user placed it meanwhile; never overwrite.
      continue;
    }
    let mut entry = archive
      .by_name(entry_name)
      .map_err(|e| format!("failed to read {entry_name}: {e}"))?;
    let mut contents = Vec::with_capacity(entry.size() as usize);
    entry
      .read_to_end(&mut contents)
      .map_err(|e| format!("failed to read {entry_name}: {e}"))?;
    fs::write(&target, contents)
      .map_err(|e| format!("failed to write {}: {e}", target.display()))?;
    placed.push(file_name.to_string());
  }

  Ok(placed)
}

fn download_verified(artifact: &PinnedArtifact) -> Result<Vec<u8>, String> {
  log_info!(
    format!("downloading {}", artifact.url),
    "external_component_setup::download_verified",
    None::<&str>
  );
  let client = reqwest::blocking::Client::builder()
    .timeout(DOWNLOAD_TIMEOUT)
    .user_agent(concat!("HardwareVisualizer/", env!("CARGO_PKG_VERSION")))
    .build()
    .map_err(|e| format!("failed to build the download client: {e}"))?;
  let response = client
    .get(artifact.url)
    .send()
    .and_then(|response| response.error_for_status())
    .map_err(|e| format!("failed to download {}: {e}", artifact.file_name))?;
  let bytes = response
    .bytes()
    .map_err(|e| format!("failed to read {}: {e}", artifact.file_name))?;

  if let Err(detail) = verify_artifact(&bytes, artifact) {
    log_warn!(
      detail.clone(),
      "external_component_setup::download_verified",
      None::<&str>
    );
    return Err(detail);
  }

  Ok(bytes.to_vec())
}

fn find_named_file(root: &Path, file_name: &str, max_depth: usize) -> Option<PathBuf> {
  if max_depth == 0 || !root.exists() {
    return None;
  }

  let direct = root.join(file_name);
  if direct.is_file() {
    return Some(direct);
  }

  let entries = fs::read_dir(root).ok()?;
  for entry in entries.flatten() {
    let path = entry.path();
    if path.is_file()
      && path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(file_name))
    {
      return Some(path);
    }
    if path.is_dir()
      && let Some(found) = find_named_file(&path, file_name, max_depth - 1)
    {
      return Some(found);
    }
  }

  None
}
