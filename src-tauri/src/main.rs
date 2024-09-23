// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#[macro_use]

mod commands;
mod enums;
mod services;
mod utils;

use commands::config;
use commands::hardware;
use services::window_menu_service;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use sysinfo::System;

fn main() {
  utils::logger::init();

  let app_state = config::AppState::new();

  let system = Arc::new(Mutex::new(System::new_all()));
  let cpu_history = Arc::new(Mutex::new(VecDeque::with_capacity(60)));
  let memory_history = Arc::new(Mutex::new(VecDeque::with_capacity(60)));
  let gpu_usage = Arc::new(Mutex::new(0.0));
  let gpu_history = Arc::new(Mutex::new(VecDeque::with_capacity(60)));

  let state = hardware::AppState {
    system: Arc::clone(&system),
    cpu_history: Arc::clone(&cpu_history),
    memory_history: Arc::clone(&memory_history),
    gpu_usage: Arc::clone(&gpu_usage),
    gpu_history: Arc::clone(&gpu_history),
  };

  let menu = window_menu_service::create_setting();

  hardware::initialize_system(
    system,
    cpu_history,
    memory_history,
    gpu_usage,
    gpu_history,
  );

  tauri::Builder::default()
    .plugin(tauri_plugin_window_state::Builder::default().build())
    .manage(state)
    .manage(app_state)
    .menu(menu)
    .invoke_handler(tauri::generate_handler![
      hardware::get_cpu_usage,
      hardware::get_hardware_info,
      hardware::get_memory_usage,
      hardware::get_gpu_usage,
      hardware::get_gpu_temperature,
      hardware::get_cpu_usage_history,
      hardware::get_memory_usage_history,
      hardware::get_gpu_usage_history,
      config::commands::set_language,
      config::commands::set_theme,
      config::commands::set_display_targets,
      config::commands::get_settings
    ])
    .on_menu_event(|event| {
      window_menu_service::handle_menu_event(event);
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
