#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use sysinfo::{Components, System};
use tauri::command;

pub struct AppState {
  pub system: Arc<Mutex<System>>,
}

///
/// システム情報の更新頻度（秒）
///
const SYSTEM_INFO_INIT_INTERVAL: u64 = 1;

///
/// ## CPU使用率（%）を取得
///
/// - pram state: `tauri::State<AppState>` アプリケーションの状態
/// - return: `i32` CPU使用率（%）
///
#[command]
pub fn get_cpu_usage(state: tauri::State<'_, AppState>) -> i32 {
  let system = state.system.lock().unwrap();
  let cpus = system.cpus();
  let total_usage: f32 = cpus.iter().map(|cpu| cpu.cpu_usage()).sum();

  let usage = total_usage / cpus.len() as f32;
  usage.round() as i32
}

///
/// ## メモリ使用率（%）を取得
///
/// - pram state: `tauri::State<AppState>` アプリケーションの状態
/// - return: `i32` メモリ使用率（%）
///
#[command]
pub fn get_memory_usage(state: tauri::State<'_, AppState>) -> i32 {
  let system = state.system.lock().unwrap();
  let used_memory = system.used_memory() as f64;
  let total_memory = system.total_memory() as f64;

  ((used_memory / total_memory) * 100.0 as f64).round() as i32
}

///
/// ## システム情報の初期化
///
/// - param system: `Arc<Mutex<System>>` システム情報
///
/// - `SYSTEM_INFO_INIT_INTERVAL` 秒ごとにCPU使用率とメモリ使用率を更新
///
pub fn initialize_system(system: Arc<Mutex<System>>) {
  thread::spawn(move || loop {
    {
      let mut sys = match system.lock() {
        Ok(s) => s,
        Err(_) => continue, // エラーハンドリング：ロックが破損している場合はスキップ
      };

      sys.refresh_cpu();
      sys.refresh_memory();
    }

    thread::sleep(Duration::from_secs(SYSTEM_INFO_INIT_INTERVAL));
  });
}
