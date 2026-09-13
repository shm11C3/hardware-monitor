use crate::enums::error::PlatformError;
use crate::platform::traits::ElevatedProcessRun;
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::OsStrExt;
use windows::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, HANDLE};
use windows::Win32::System::Threading::{
  GetExitCodeProcess, INFINITE, WaitForSingleObject,
};
use windows::Win32::UI::Shell::{
  IsUserAnAdmin, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
  ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::PCWSTR;

pub fn is_process_elevated() -> Result<bool, PlatformError> {
  Ok(unsafe { IsUserAnAdmin().as_bool() })
}

pub fn relaunch_current_process_elevated() -> Result<(), PlatformError> {
  let args = std::env::args_os().skip(1).collect::<Vec<_>>();
  let launched = launch_current_executable_elevated(&args, "restart as administrator")?;

  match launched {
    Some(process) => {
      let _ = unsafe { CloseHandle(process) };
      Ok(())
    }
    // The existing restart contract reports a declined UAC prompt as a failure
    // so the caller can roll back Elevated Startup Mode.
    None => Err(PlatformError::fault(
      "Failed to restart as administrator: the elevation prompt was declined",
    )),
  }
}

/// Launch the current executable elevated with `args`, wait for it to exit,
/// and return its exit code. A declined UAC prompt is reported as
/// [`ElevatedProcessRun::Declined`], not as an error.
pub fn run_current_executable_elevated(
  args: &[String],
) -> Result<ElevatedProcessRun, PlatformError> {
  let args = args.iter().map(OsString::from).collect::<Vec<_>>();
  let Some(process) = launch_current_executable_elevated(&args, "run as administrator")?
  else {
    return Ok(ElevatedProcessRun::Declined);
  };

  let exit_code = wait_for_exit_code(process);
  let _ = unsafe { CloseHandle(process) };
  Ok(ElevatedProcessRun::Exited { exit_code })
}

/// Returns the process handle, or `None` when the user declined the prompt.
fn launch_current_executable_elevated(
  args: &[OsString],
  action: &str,
) -> Result<Option<HANDLE>, PlatformError> {
  let exe_path = std::env::current_exe().map_err(|e| {
    PlatformError::fault(format!("Failed to obtain executable file path: {e}"))
  })?;
  let params = args
    .iter()
    .map(|arg| quote_windows_arg(arg))
    .collect::<Vec<_>>()
    .join(" ");

  let verb = wide_null("runas");
  let file = os_wide_null(exe_path.as_os_str());
  let parameters = os_wide_null(OsString::from(params).as_os_str());

  let mut execute_info = SHELLEXECUTEINFOW {
    cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
    fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
    lpVerb: PCWSTR(verb.as_ptr()),
    lpFile: PCWSTR(file.as_ptr()),
    lpParameters: PCWSTR(parameters.as_ptr()),
    nShow: SW_SHOWNORMAL.0,
    ..Default::default()
  };

  if let Err(e) = unsafe { ShellExecuteExW(&mut execute_info) } {
    if e.code() == ERROR_CANCELLED.to_hresult() {
      return Ok(None);
    }
    return Err(PlatformError::fault(format!("Failed to {action}: {e}")));
  }

  if execute_info.hProcess.is_invalid() {
    return Err(PlatformError::fault(format!(
      "Failed to {action}: no process handle was returned"
    )));
  }

  Ok(Some(execute_info.hProcess))
}

fn wait_for_exit_code(process: HANDLE) -> Option<i32> {
  let _ = unsafe { WaitForSingleObject(process, INFINITE) };
  let mut exit_code: u32 = 0;
  unsafe { GetExitCodeProcess(process, &mut exit_code) }
    .ok()
    .map(|()| exit_code as i32)
}

fn os_wide_null(value: &OsStr) -> Vec<u16> {
  value.encode_wide().chain(std::iter::once(0)).collect()
}

fn wide_null(value: &str) -> Vec<u16> {
  value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn quote_windows_arg(arg: &OsStr) -> String {
  let value = arg.to_string_lossy();

  if value.is_empty() {
    return "\"\"".to_string();
  }

  if !value
    .chars()
    .any(|ch| matches!(ch, ' ' | '\t' | '\n' | '\u{000b}' | '"'))
  {
    return value.into_owned();
  }

  let mut quoted = String::from("\"");
  let mut backslashes = 0;

  for ch in value.chars() {
    if ch == '\\' {
      backslashes += 1;
      continue;
    }

    if ch == '"' {
      quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
      quoted.push('"');
      backslashes = 0;
      continue;
    }

    if backslashes > 0 {
      quoted.push_str(&"\\".repeat(backslashes));
      backslashes = 0;
    }
    quoted.push(ch);
  }

  if backslashes > 0 {
    quoted.push_str(&"\\".repeat(backslashes * 2));
  }

  quoted.push('"');
  quoted
}

#[cfg(test)]
mod tests {
  use super::quote_windows_arg;
  use std::ffi::OsStr;

  #[test]
  fn quote_windows_arg_preserves_simple_arguments() {
    assert_eq!(quote_windows_arg(OsStr::new("--flag")), "--flag");
  }

  #[test]
  fn quote_windows_arg_quotes_spaces_and_quotes() {
    assert_eq!(
      quote_windows_arg(OsStr::new(r#"--path=C:\Program Files\"quoted""#)),
      r#""--path=C:\Program Files\\\"quoted\"""#
    );
  }
}
