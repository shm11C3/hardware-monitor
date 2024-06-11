// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod get_hardware_data;
mod get_setting_data;
mod windows_service;

use get_hardware_data::{
  get_cpu_usage, get_cpu_usage_history, get_gpu_usage, get_gpu_usage_history, get_memory_usage, get_memory_usage_history, initialize_system, AppState,
};

use get_setting_data::get_windows_theme_mode;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use sysinfo::System;

fn main() {
  let system = Arc::new(Mutex::new(System::new_all()));
  let cpu_history = Arc::new(Mutex::new(VecDeque::with_capacity(60)));
  let memory_history = Arc::new(Mutex::new(VecDeque::with_capacity(60)));
  let gpu_usage = Arc::new(Mutex::new(0.0));
  let gpu_history = Arc::new(Mutex::new(VecDeque::with_capacity(60)));

  let state = AppState {
    system: Arc::clone(&system),
    cpu_history: Arc::clone(&cpu_history),
    memory_history: Arc::clone(&memory_history),
    gpu_usage: Arc::clone(&gpu_usage),
    gpu_history: Arc::clone(&gpu_history),
  };

  initialize_system(system, cpu_history, memory_history, gpu_usage, gpu_history);

  tauri::Builder::default()
    .manage(state)
    .invoke_handler(tauri::generate_handler![
      get_cpu_usage,
      get_memory_usage,
      get_gpu_usage,
      get_cpu_usage_history,
      get_memory_usage_history,
      get_gpu_usage_history,
      get_windows_theme_mode
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
