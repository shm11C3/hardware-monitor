#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
  hardware_monitor_lib::run();
}
