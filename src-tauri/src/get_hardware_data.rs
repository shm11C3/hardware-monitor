#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use sysinfo::System;
use tauri::command;

pub struct AppState {
   pub system: Arc<Mutex<System>>,
}

#[command]
pub fn get_cpu(state: tauri::State<'_, AppState>) -> f32 {
   let system = state.system.lock().unwrap();
   let cpus = system.cpus();
   let total_usage: f32 = cpus.iter().map(|cpu| cpu.cpu_usage()).sum();
   total_usage / cpus.len() as f32
}

pub fn initialize_system(system: Arc<Mutex<System>>) {
   thread::spawn(move || loop {
      {
         let mut sys = system.lock().unwrap();
         sys.refresh_cpu();
      }
      thread::sleep(Duration::from_secs(1));
   });
}
