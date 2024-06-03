// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod get_hardware_data;

use get_hardware_data::{get_cpu_usage, get_memory_usage, initialize_system, AppState};
use std::sync::{Arc, Mutex};
use sysinfo::System;

fn main() {
  let system = Arc::new(Mutex::new(System::new_all()));
  let state = AppState { system: Arc::clone(&system) };

  initialize_system(system);

  tauri::Builder::default()
    .manage(state)
    .invoke_handler(tauri::generate_handler![get_cpu_usage, get_memory_usage])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
