#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
  if let Some(exit_code) = hardware_monitor_lib::run_cli_mode_if_requested() {
    std::process::exit(exit_code);
  }
  hardware_monitor_lib::run();
}
