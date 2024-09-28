use crate::services::graphic_service;
use crate::services::system_info_service;
use crate::{log_debug, log_error, log_internal, log_warn};
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use sysinfo::System;
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SysInfo {
  pub cpu: Option<system_info_service::CpuInfo>,
  pub memory: Option<system_info_service::MemoryInfo>,
  pub gpus: Option<Vec<graphic_service::GraphicInfo>>,
}

///
/// ## システム情報を取得
///
#[command]
pub async fn get_hardware_info(
  state: tauri::State<'_, AppState>,
) -> Result<SysInfo, String> {
  let cpu_result = system_info_service::get_cpu_info(state.system.lock().unwrap());
  let memory_result = system_info_service::get_memory_info();
  let gpus_result = graphic_service::get_nvidia_gpu_info().await;

  let sys_info = SysInfo {
    cpu: cpu_result.ok(),
    memory: memory_result.ok(),
    gpus: gpus_result.ok(),
  };

  // すべての情報が失敗した場合にのみエラーメッセージを返す
  if sys_info.cpu.is_none() && sys_info.memory.is_none() && sys_info.gpus.is_none() {
    Err("Failed to get any hardware info".to_string())
  } else {
    Ok(sys_info)
  }
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
pub async fn get_gpu_usage() -> Result<i32, String> {
  match graphic_service::get_nvidia_gpu_usage().await {
    Ok(usage) => Ok((usage * 100.0).round() as i32),
    Err(e) => Err(format!("Failed to get GPU usage: {:?}", e)),
  }
}

///
/// ## GPU温度を取得
///
#[command]
pub async fn get_gpu_temperature() -> Result<Vec<graphic_service::NameValue>, String> {
  match graphic_service::get_nvidia_gpu_temperature().await {
    Ok(temps) => Ok(temps),
    Err(e) => Err(format!("Failed to get GPU temperature: {:?}", e)),
  }
}

///
/// ## GPUのファン回転数を取得
///
#[command]
pub async fn get_nvidia_gpu_cooler() -> Result<Vec<graphic_service::NameValue>, String> {
  match graphic_service::get_nvidia_gpu_cooler_stat().await {
    Ok(temps) => Ok(temps),
    Err(e) => Err(format!("Failed to get GPU cooler status: {:?}", e)),
  }
}

///
/// ## CPU使用率の履歴を取得
///
/// - param state: `tauri::State<AppState>` アプリケーションの状態
/// - param seconds: `usize` 取得する秒数
///
#[command]
pub fn get_cpu_usage_history(
  state: tauri::State<'_, AppState>,
  seconds: usize,
) -> Vec<f32> {
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
pub fn get_memory_usage_history(
  state: tauri::State<'_, AppState>,
  seconds: usize,
) -> Vec<f32> {
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
pub fn get_gpu_usage_history(
  state: tauri::State<'_, AppState>,
  seconds: usize,
) -> Vec<f32> {
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
pub fn initialize_system(
  system: Arc<Mutex<System>>,
  cpu_history: Arc<Mutex<VecDeque<f32>>>,
  memory_history: Arc<Mutex<VecDeque<f32>>>,
  gpu_usage: Arc<Mutex<f32>>,
  gpu_history: Arc<Mutex<VecDeque<f32>>>,
) {
  thread::spawn(move || loop {
    {
      let mut sys = match system.lock() {
        Ok(s) => s,
        Err(_) => continue, // エラーハンドリング：ロックが破損している場合はスキップ
      };

      sys.refresh_cpu_all();
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

      //let gpu_usage_value = match get_gpu_usage() {
      //  Ok(usage) => usage,
      //  Err(_) => 0.0, // エラーが発生した場合はデフォルト値として0.0を使用
      //};

      //{
      //  let mut gpu = gpu_usage.lock().unwrap();
      //  *gpu = gpu_usage_value;
      //}

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

      //{
      //  let mut gpu_hist = gpu_history.lock().unwrap();
      //  if gpu_hist.len() >= HISTORY_CAPACITY {
      //    gpu_hist.pop_front();
      //  }
      //  gpu_hist.push_back(gpu_usage_value);
      //}
    }

    thread::sleep(Duration::from_secs(SYSTEM_INFO_INIT_INTERVAL));
  });
}
