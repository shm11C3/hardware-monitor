//! Windows execution of an External Component Setup plan.
//!
//! This module runs inside an already elevated process: the App launches the
//! executable elevated in its setup command-line mode, or the installer's
//! custom action calls it from its elevated context. It downloads the pinned
//! artifacts, verifies them, runs the runtime installer unattended, and places
//! only the module files that are missing. It never overwrites or removes
//! anything, and it reports back through the process exit code only.
//!
//! Elevation-boundary rules this file keeps:
//!
//! - The verified installer is staged in an administrator-only directory
//!   under `%SystemRoot%\Temp`, not in the user's temp directory, and is held
//!   open with an exclusive share mode while it runs, so a same-user
//!   medium-integrity process cannot swap it between verification and start.
//! - Module files are published atomically with no-clobber semantics, so a
//!   partial file never counts as present and a concurrent writer is never
//!   truncated.
//! - Enumeration failures are reported as unknown state, never as absence.

use std::ffi::OsStr;
use std::fs;
use std::io::{Cursor, Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use windows::Win32::Foundation::{
  ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, HLOCAL, LocalFree,
};
use windows::Win32::Security::Authorization::{
  ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::{CreateDirectoryW, FILE_SHARE_READ};
use windows::Win32::System::Registry::{
  HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, REG_SZ, REG_VALUE_TYPE,
  RegCloseKey, RegOpenKeyExW, RegQueryValueExW,
};
use windows::core::PCWSTR;

use super::{
  ExternalComponentSetupOutcome, ExternalComponentSetupPlan,
  ExternalComponentSetupResult, ExternalComponentSetupStatus,
  ExternalComponentSetupSupport, FileBundleStep, InstallerExitOutcome, ModuleFileState,
  PinnedArtifact, RuntimeInstallState, SetupFailureStage, interpret_installer_exit_code,
  select_bundle_entries, verify_artifact,
};
use crate::models::ExternalComponent;
use crate::{log_info, log_warn};

/// Uninstall registry key the PawnIO installer registers; `InstallLocation`
/// is the documented discovery path (`docs/specs/sensors/pawnio-interface.md`).
const PAWNIO_UNINSTALL_SUBKEY: &str =
  r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\PawnIO";
const PAWNIO_DIRECTORY_NAME: &str = "PawnIO";
/// Matches the provider's recursive module search depth so "present" here
/// agrees with what collection would find.
const MODULE_SEARCH_DEPTH: usize = 4;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);
/// Protected DACL: full control for Administrators and SYSTEM, nothing for
/// anyone else, no inheritance from `%SystemRoot%\Temp`.
const STAGING_DIRECTORY_SDDL: &str = "D:P(A;OICI;FA;;;BA)(A;OICI;FA;;;SY)";
const PARTIAL_SUFFIX: &str = ".hardviz-partial";

pub fn status(plan: &ExternalComponentSetupPlan) -> ExternalComponentSetupStatus {
  let runtime = runtime_install_state(plan.component);
  let roots = install_roots(&runtime);
  let mut enumeration_error = None;
  let module_files = plan
    .file_bundle
    .file_names
    .iter()
    .map(|file_name| {
      let mut present = false;
      for root in &roots {
        match find_named_file(root, file_name, MODULE_SEARCH_DEPTH) {
          Ok(Some(_)) => {
            present = true;
            break;
          }
          Ok(None) => {}
          Err(e) => {
            enumeration_error.get_or_insert_with(|| format!("{}: {e}", root.display()));
          }
        }
      }
      ModuleFileState {
        file_name: (*file_name).to_string(),
        present,
      }
    })
    .collect();

  ExternalComponentSetupStatus {
    component: plan.component,
    support: ExternalComponentSetupSupport::Supported,
    runtime,
    module_files,
    enumeration_error,
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
  if let Some(blocker) = before.setup_blocker() {
    result.outcome =
      ExternalComponentSetupOutcome::failed(SetupFailureStage::StateUnknown, blocker);
    return result;
  }
  if before.is_complete() {
    return result;
  }

  let mut reboot_required = false;
  if matches!(before.runtime, RuntimeInstallState::NotInstalled) {
    let staging = match StagingDirectory::create() {
      Ok(staging) => staging,
      Err(detail) => {
        result.outcome = ExternalComponentSetupOutcome::failed(
          SetupFailureStage::StagingDirectory,
          detail,
        );
        return result;
      }
    };
    match install_runtime(plan, &staging.path) {
      Ok(InstallerExitOutcome::Installed) => result.runtime_installed = true,
      Ok(InstallerExitOutcome::RebootRequired) => {
        result.runtime_installed = true;
        reboot_required = true;
      }
      Ok(InstallerExitOutcome::Failed(code)) => {
        result.outcome = ExternalComponentSetupOutcome::failed(
          SetupFailureStage::InstallerExit,
          format!(
            "{} exited with {}",
            plan.installer.artifact.file_name,
            code.map_or_else(|| "no exit code".to_string(), |code| code.to_string())
          ),
        );
        return result;
      }
      Err((stage, detail)) => {
        result.outcome = ExternalComponentSetupOutcome::failed(stage, detail);
        return result;
      }
    }
  }

  let after = status(plan);
  if let Some(blocker) = after.setup_blocker() {
    result.outcome =
      ExternalComponentSetupOutcome::failed(SetupFailureStage::StateUnknown, blocker);
    return result;
  }
  let missing = after.missing_module_files();
  if !missing.is_empty() {
    let destination = module_destination(&after.runtime);
    match place_module_files(&plan.file_bundle, &missing, &destination) {
      Ok(placed) => result.module_files_placed = placed,
      Err((stage, detail)) => {
        result.outcome = ExternalComponentSetupOutcome::failed(stage, detail);
        return result;
      }
    }
  }

  // Do not report success on the strength of the steps alone; the state after
  // the run is the only evidence the caller can act on.
  let final_state = status(plan);
  if !final_state.is_complete() && !reboot_required {
    let detail = final_state.setup_blocker().unwrap_or_else(|| {
      format!(
        "runtime {} and {} module file(s) still missing",
        match final_state.runtime {
          RuntimeInstallState::Installed { .. } => "installed",
          _ => "not registered",
        },
        final_state.missing_module_files().len()
      )
    });
    result.outcome =
      ExternalComponentSetupOutcome::failed(SetupFailureStage::Incomplete, detail);
    return result;
  }

  result.outcome = if reboot_required {
    ExternalComponentSetupOutcome::RebootRequired
  } else {
    ExternalComponentSetupOutcome::Installed
  };
  result
}

fn runtime_install_state(component: ExternalComponent) -> RuntimeInstallState {
  let subkey = match component {
    ExternalComponent::Pawnio => PAWNIO_UNINSTALL_SUBKEY,
    ExternalComponent::Smartctl => return RuntimeInstallState::NotInstalled,
  };

  let subkey_w = wide_null(subkey);
  let mut key = HKEY::default();
  let opened = unsafe {
    RegOpenKeyExW(
      HKEY_LOCAL_MACHINE,
      PCWSTR(subkey_w.as_ptr()),
      Some(0),
      KEY_READ | KEY_WOW64_64KEY,
      &mut key,
    )
  };
  if opened == ERROR_FILE_NOT_FOUND {
    return RuntimeInstallState::NotInstalled;
  }
  if opened != ERROR_SUCCESS {
    return RuntimeInstallState::Unknown {
      detail: format!("RegOpenKeyExW({subkey}) failed with {}", opened.0),
    };
  }

  let version = read_registry_string(key, "DisplayVersion");
  let install_location = read_registry_string(key, "InstallLocation").map(PathBuf::from);
  let _ = unsafe { RegCloseKey(key) };

  RuntimeInstallState::Installed {
    version,
    install_location,
  }
}

fn read_registry_string(key: HKEY, value_name: &str) -> Option<String> {
  let name_w = wide_null(value_name);
  let mut kind = REG_VALUE_TYPE::default();
  let mut size: u32 = 0;
  let probed = unsafe {
    RegQueryValueExW(
      key,
      PCWSTR(name_w.as_ptr()),
      None,
      Some(&mut kind),
      None,
      Some(&mut size),
    )
  };
  if probed != ERROR_SUCCESS || kind != REG_SZ || size == 0 {
    return None;
  }

  let mut buffer = vec![0u8; size as usize];
  let read = unsafe {
    RegQueryValueExW(
      key,
      PCWSTR(name_w.as_ptr()),
      None,
      Some(&mut kind),
      Some(buffer.as_mut_ptr()),
      Some(&mut size),
    )
  };
  if read != ERROR_SUCCESS {
    return None;
  }

  let (pairs, _) = buffer[..size as usize].as_chunks::<2>();
  let units = pairs
    .iter()
    .map(|pair| u16::from_le_bytes(*pair))
    .collect::<Vec<_>>();
  let end = units
    .iter()
    .position(|&unit| unit == 0)
    .unwrap_or(units.len());
  let value = String::from_utf16_lossy(&units[..end]);
  let value = value.trim();
  (!value.is_empty()).then(|| value.to_string())
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

/// An administrator-only directory under `%SystemRoot%\Temp`, removed on
/// drop. `%SystemRoot%\Temp` lets standard users create entries but not list,
/// delete, or rename them, and the protected DACL keeps the contents
/// unreadable and unwritable for medium-integrity processes.
struct StagingDirectory {
  path: PathBuf,
}

impl StagingDirectory {
  fn create() -> Result<Self, String> {
    let system_root = std::env::var_os("SystemRoot")
      .ok_or_else(|| "SystemRoot is not set".to_string())?;
    let mut random = [0u8; 16];
    getrandom::fill(&mut random).map_err(|e| format!("random name failed: {e}"))?;
    let name = format!(
      "hardviz-external-component-setup-{}",
      random
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
    );
    let path = PathBuf::from(system_root).join("Temp").join(name);

    let sddl = wide_null(STAGING_DIRECTORY_SDDL);
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe {
      ConvertStringSecurityDescriptorToSecurityDescriptorW(
        PCWSTR(sddl.as_ptr()),
        SDDL_REVISION_1,
        &mut descriptor,
        None,
      )
    }
    .map_err(|e| format!("security descriptor failed: {e}"))?;
    let attributes = SECURITY_ATTRIBUTES {
      nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
      lpSecurityDescriptor: descriptor.0,
      bInheritHandle: false.into(),
    };
    let path_w = os_wide_null(path.as_os_str());
    let created = unsafe { CreateDirectoryW(PCWSTR(path_w.as_ptr()), Some(&attributes)) };
    let _ = unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
    created.map_err(|e| format!("failed to create {}: {e}", path.display()))?;

    Ok(Self { path })
  }
}

impl Drop for StagingDirectory {
  fn drop(&mut self) {
    let _ = fs::remove_dir_all(&self.path);
  }
}

fn install_runtime(
  plan: &ExternalComponentSetupPlan,
  staging: &Path,
) -> Result<InstallerExitOutcome, (SetupFailureStage, String)> {
  let artifact = &plan.installer.artifact;
  let bytes = download_verified(artifact).map_err(|(stage, detail)| {
    (
      match stage {
        DownloadFailure::Transfer => SetupFailureStage::DownloadRuntime,
        DownloadFailure::Verification => SetupFailureStage::VerifyRuntime,
      },
      detail,
    )
  })?;
  let installer_path = staging.join(artifact.file_name);

  // Create exclusively, write, then hold a read handle that denies write and
  // delete sharing for as long as the installer runs. The loader opens the
  // image for read/execute, which this share mode allows.
  {
    let mut file = fs::OpenOptions::new()
      .write(true)
      .create_new(true)
      .share_mode(0)
      .open(&installer_path)
      .map_err(|e| {
        (
          SetupFailureStage::StagingDirectory,
          format!("failed to create {}: {e}", installer_path.display()),
        )
      })?;
    file
      .write_all(&bytes)
      .and_then(|()| file.sync_all())
      .map_err(|e| {
        (
          SetupFailureStage::StagingDirectory,
          format!("failed to write {}: {e}", installer_path.display()),
        )
      })?;
  }
  let mut guard = fs::OpenOptions::new()
    .read(true)
    .share_mode(FILE_SHARE_READ.0)
    .open(&installer_path)
    .map_err(|e| {
      (
        SetupFailureStage::StagingDirectory,
        format!("failed to reopen {}: {e}", installer_path.display()),
      )
    })?;
  let mut staged = Vec::with_capacity(bytes.len());
  guard.read_to_end(&mut staged).map_err(|e| {
    (
      SetupFailureStage::VerifyRuntime,
      format!("failed to read back {}: {e}", installer_path.display()),
    )
  })?;
  verify_artifact(&staged, artifact)
    .map_err(|detail| (SetupFailureStage::VerifyRuntime, detail))?;

  log_info!(
    format!(
      "running {} {}",
      artifact.file_name,
      plan.installer.unattended_args.join(" ")
    ),
    "external_component_setup::install_runtime",
    None::<&str>
  );
  let exit = Command::new(&installer_path)
    .args(plan.installer.unattended_args)
    .status()
    .map_err(|e| {
      (
        SetupFailureStage::StartInstaller,
        format!("failed to start {}: {e}", artifact.file_name),
      )
    })?;
  drop(guard);

  Ok(interpret_installer_exit_code(&plan.installer, exit.code()))
}

fn place_module_files(
  bundle: &FileBundleStep,
  missing: &[&str],
  destination: &Path,
) -> Result<Vec<String>, (SetupFailureStage, String)> {
  let bytes = download_verified(&bundle.artifact).map_err(|(stage, detail)| {
    (
      match stage {
        DownloadFailure::Transfer => SetupFailureStage::DownloadModules,
        DownloadFailure::Verification => SetupFailureStage::VerifyModules,
      },
      detail,
    )
  })?;
  let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| {
    (
      SetupFailureStage::ArchiveContents,
      format!("failed to open {}: {e}", bundle.artifact.file_name),
    )
  })?;
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
    return Err((
      SetupFailureStage::ArchiveContents,
      format!(
        "{} does not contain {}",
        bundle.artifact.file_name,
        absent.join(", ")
      ),
    ));
  }

  fs::create_dir_all(destination).map_err(|e| {
    (
      SetupFailureStage::PlaceModules,
      format!("failed to create {}: {e}", destination.display()),
    )
  })?;

  let mut placed = Vec::new();
  for (entry_name, file_name) in selected {
    let mut entry = archive.by_name(entry_name).map_err(|e| {
      (
        SetupFailureStage::ArchiveContents,
        format!("failed to read {entry_name}: {e}"),
      )
    })?;
    let mut contents = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut contents).map_err(|e| {
      (
        SetupFailureStage::ArchiveContents,
        format!("failed to read {entry_name}: {e}"),
      )
    })?;
    if publish_file_no_clobber(destination, file_name, &contents)
      .map_err(|detail| (SetupFailureStage::PlaceModules, detail))?
    {
      placed.push(file_name.to_string());
    }
  }

  Ok(placed)
}

/// Write `contents` to `<directory>\<file_name>` atomically without ever
/// replacing an existing file. The data is staged in a sibling partial file
/// and linked into place; a link fails when the target already exists, so a
/// concurrent placement or a user-provided file is preserved untouched and a
/// partial file never appears under the final name. Returns whether the file
/// was placed by this call.
fn publish_file_no_clobber(
  directory: &Path,
  file_name: &str,
  contents: &[u8],
) -> Result<bool, String> {
  let target = directory.join(file_name);
  let partial = directory.join(format!("{file_name}{PARTIAL_SUFFIX}"));

  let mut file = fs::OpenOptions::new()
    .write(true)
    .create_new(true)
    .open(&partial)
    .map_err(|e| format!("failed to create {}: {e}", partial.display()))?;
  let written = file.write_all(contents).and_then(|()| file.sync_all());
  drop(file);
  if let Err(e) = written {
    let _ = fs::remove_file(&partial);
    return Err(format!("failed to write {}: {e}", partial.display()));
  }

  let linked = fs::hard_link(&partial, &target);
  let _ = fs::remove_file(&partial);
  match linked {
    Ok(()) => Ok(true),
    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
    Err(e) => Err(format!("failed to place {}: {e}", target.display())),
  }
}

enum DownloadFailure {
  Transfer,
  Verification,
}

fn download_verified(
  artifact: &PinnedArtifact,
) -> Result<Vec<u8>, (DownloadFailure, String)> {
  log_info!(
    format!("downloading {}", artifact.url),
    "external_component_setup::download_verified",
    None::<&str>
  );
  let client = reqwest::blocking::Client::builder()
    .timeout(DOWNLOAD_TIMEOUT)
    .user_agent(concat!("HardwareVisualizer/", env!("CARGO_PKG_VERSION")))
    .build()
    .map_err(|e| {
      (
        DownloadFailure::Transfer,
        format!("failed to build the download client: {e}"),
      )
    })?;
  let response = client
    .get(artifact.url)
    .send()
    .and_then(|response| response.error_for_status())
    .map_err(|e| {
      (
        DownloadFailure::Transfer,
        format!("failed to download {}: {e}", artifact.file_name),
      )
    })?;
  let bytes = response.bytes().map_err(|e| {
    (
      DownloadFailure::Transfer,
      format!("failed to read {}: {e}", artifact.file_name),
    )
  })?;

  if let Err(detail) = verify_artifact(&bytes, artifact) {
    log_warn!(
      detail.clone(),
      "external_component_setup::download_verified",
      None::<&str>
    );
    return Err((DownloadFailure::Verification, detail));
  }

  Ok(bytes.to_vec())
}

/// Search `root` for `file_name` up to `max_depth` levels. A missing root is
/// positive absence; a directory that cannot be read is an error, because a
/// permission or transient failure is not evidence that the file is absent.
fn find_named_file(
  root: &Path,
  file_name: &str,
  max_depth: usize,
) -> std::io::Result<Option<PathBuf>> {
  if max_depth == 0 || !root.exists() {
    return Ok(None);
  }

  let direct = root.join(file_name);
  if direct.is_file() {
    return Ok(Some(direct));
  }

  for entry in fs::read_dir(root)? {
    let path = entry?.path();
    if path.is_file()
      && path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(file_name))
    {
      return Ok(Some(path));
    }
    if path.is_dir()
      && let Some(found) = find_named_file(&path, file_name, max_depth - 1)?
    {
      return Ok(Some(found));
    }
  }

  Ok(None)
}

fn wide_null(value: &str) -> Vec<u16> {
  value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn os_wide_null(value: &OsStr) -> Vec<u16> {
  value.encode_wide().chain(std::iter::once(0)).collect()
}
