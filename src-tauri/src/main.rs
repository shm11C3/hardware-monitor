// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod get_hardware_data;

use get_hardware_data::{get_cpu_usage, get_cpu_usage_history, get_memory_usage, get_memory_usage_history, initialize_system, AppState};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use sysinfo::System;

fn main() {
  let system = Arc::new(Mutex::new(System::new_all()));
  let cpu_history = Arc::new(Mutex::new(VecDeque::with_capacity(60)));
  let memory_history = Arc::new(Mutex::new(VecDeque::with_capacity(60)));

  let state = AppState {
    system: Arc::clone(&system),
    cpu_history: Arc::clone(&cpu_history),
    memory_history: Arc::clone(&memory_history),
  };

  initialize_system(system, cpu_history, memory_history);

  tauri::Builder::default()
    .manage(state)
    .invoke_handler(tauri::generate_handler![
      get_cpu_usage,
      get_memory_usage,
      get_cpu_usage_history,
      get_memory_usage_history
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
