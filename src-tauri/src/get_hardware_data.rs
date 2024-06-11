#![cfg_attr(not(debug_assertions), target_os = "windows")]

use std::collections::VecDeque;
use std::process;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use sysinfo::{Components, System};
use tauri::command;

pub struct AppState { 
  pub system: Arc<Mutex<System>>,
  pub cpu_history: Arc<Mutex<VecDeque<f32>>>,
  pub memory_history: Arc<Mutex<VecDeque<f32>>>,
  pub gpu_history: Arc<Mutex<VecDeque<f32>>>,
  pub gpu_usage: Arc<Mutex<f32>>,
}

///
/// システム情報の更新頻度（秒）
///
const SYSTEM_INFO_INIT_INTERVAL: u64 = 1;

///
/// データを保持する期間（秒）
///
const HISTORY_CAPACITY: usize = 60;

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
/// ## GPU使用率（%）を取得（Nvidia 限定）
///
/// - param state: `tauri::State<AppState>` アプリケーションの状態
/// - return: `i32` GPU使用率（%）
///
#[command]
pub fn get_gpu_usage(state: tauri::State<'_, AppState>) -> i32 {
  let gpu_usage = state.gpu_usage.lock().unwrap();
  *gpu_usage as i32
}

///
/// ## CPU使用率の履歴を取得
///
/// - param state: `tauri::State<AppState>` アプリケーションの状態
/// - param seconds: `usize` 取得する秒数
///
#[command]
pub fn get_cpu_usage_history(state: tauri::State<'_, AppState>, seconds: usize) -> Vec<f32> {
  let history = state.cpu_history.lock().unwrap();
  history.iter().rev().take(seconds).cloned().collect()
}

///
/// ## メモリ使用率の履歴を取得
///
/// - param state: `tauri::State<AppState>` アプリケーションの状態
/// - param seconds: `usize` 取得する秒数
///
#[command]
pub fn get_memory_usage_history(state: tauri::State<'_, AppState>, seconds: usize) -> Vec<f32> {
  let history = state.memory_history.lock().unwrap();
  history.iter().rev().take(seconds).cloned().collect()
}


///
/// ## GPU使用率の履歴を取得
///
/// - param state: `tauri::State<AppState>` アプリケーションの状態
/// - param seconds: `usize` 取得する秒数
///
#[command]
pub fn get_gpu_usage_history(state: tauri::State<'_, AppState>, seconds: usize) -> Vec<f32> {
  let history = state.gpu_history.lock().unwrap();
  history.iter().rev().take(seconds).cloned().collect()
}

///
/// ## システム情報の初期化
///
/// - param system: `Arc<Mutex<System>>` システム情報
///
/// - `SYSTEM_INFO_INIT_INTERVAL` 秒ごとにCPU使用率とメモリ使用率を更新
///
pub fn initialize_system(system: Arc<Mutex<System>>, cpu_history: Arc<Mutex<VecDeque<f32>>>, memory_history: Arc<Mutex<VecDeque<f32>>>, gpu_usage: Arc<Mutex<f32>>, gpu_history: Arc<Mutex<VecDeque<f32>>>) {
  thread::spawn(move || loop {
    {
      let mut sys = match system.lock() {
        Ok(s) => s,
        Err(_) => continue, // エラーハンドリング：ロックが破損している場合はスキップ
      };

      sys.refresh_cpu();
      sys.refresh_memory();

      let cpu_usage = {
        let cpus = sys.cpus();
        let total_usage: f32 = cpus.iter().map(|cpu| cpu.cpu_usage()).sum();
        (total_usage / cpus.len() as f32).round() as f32
      };

      let memory_usage = {
        let used_memory = sys.used_memory() as f64;
        let total_memory = sys.total_memory() as f64;
        (used_memory / total_memory * 100.0).round() as f32
      };

      let gpu_usage_value = {
        let output = process::Command::new("nvidia-smi")
            .arg("--query-gpu=utilization.gpu")
            .arg("--format=csv,noheader,nounits")
            .output()
            .expect("failed to execute process");

        let usage_str = String::from_utf8_lossy(&output.stdout);
        usage_str.trim().parse().unwrap_or(0.0)
    };

    {
        let mut gpu = gpu_usage.lock().unwrap();
        *gpu = gpu_usage_value;
    }

      {
        let mut cpu_hist = cpu_history.lock().unwrap();
        if cpu_hist.len() >= HISTORY_CAPACITY {
          cpu_hist.pop_front();
        }
        cpu_hist.push_back(cpu_usage);
      }

      {
        let mut memory_hist = memory_history.lock().unwrap();
        if memory_hist.len() >= HISTORY_CAPACITY {
          memory_hist.pop_front();
        }
        memory_hist.push_back(memory_usage);
      }

      {
        let mut gpu_hist = gpu_history.lock().unwrap();
        if gpu_hist.len() >= HISTORY_CAPACITY {
            gpu_hist.pop_front();
        }
        gpu_hist.push_back(gpu_usage_value);
    }
    }

    thread::sleep(Duration::from_secs(SYSTEM_INFO_INIT_INTERVAL));
  });
}
