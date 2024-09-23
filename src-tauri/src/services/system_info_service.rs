use crate::utils::{self, formatter};
use crate::{log_debug, log_error, log_info, log_internal};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::MutexGuard;
use std::thread;
use sysinfo::System;
use wmi::{COMLibrary, WMIConnection};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuInfo {
  name: String,
  vendor: String,
  core_count: usize,
  clock: u64,
  clock_unit: String,
  cpu_name: String,
}

///
/// ## CPU情報を取得
///
pub fn get_cpu_info(system: MutexGuard<'_, System>) -> Result<CpuInfo, String> {
  let cpus = system.cpus();

  if cpus.is_empty() {
    return Err("CPU information not available".to_string());
  }

  // CPU情報を収集
  let cpu_info = CpuInfo {
    name: cpus[0].brand().to_string(),
    vendor: utils::formatter::format_vendor_name(cpus[0].vendor_id()),
    core_count: cpus.len(),
    clock: cpus[0].frequency(),
    clock_unit: "MHz".to_string(),
    cpu_name: cpus[0].name().to_string(),
  };

  Ok(cpu_info)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInfo {
  size: String,
  clock: u64,
  clock_unit: String,
  memory_count: usize,
  total_slots: usize,
  memory_type: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Win32PhysicalMemory {
  capacity: u64,
  speed: u32,
  memory_type: Option<u16>,
  smbios_memory_type: Option<u16>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Win32PhysicalMemoryArray {
  memory_devices: Option<u32>,
}

///
/// ## メモリ情報を取得
///
pub fn get_memory_info() -> Result<MemoryInfo, String> {
  let physical_memory: Vec<Win32PhysicalMemory> = get_memory_info_in_thread(
    "SELECT Capacity, Speed, MemoryType, SMBIOSMemoryType FROM Win32_PhysicalMemory"
      .to_string(),
  )?;

  let physical_memory_array: Vec<Win32PhysicalMemoryArray> = get_memory_info_in_thread(
    "SELECT MemoryDevices FROM Win32_PhysicalMemoryArray".to_string(),
  )?;

  log_info!(
    &format!("mem info: {:?}", physical_memory),
    "get_memory_info",
    None::<&str>
  );

  let memory_info = MemoryInfo {
    size: formatter::format_size(physical_memory.iter().map(|mem| mem.capacity).sum(), 1),
    clock: physical_memory[0].speed as u64,
    clock_unit: "MHz".to_string(),
    memory_count: physical_memory.len(),
    total_slots: physical_memory_array[0].memory_devices.unwrap_or(0) as usize,
    memory_type: get_memory_type_with_fallback(
      physical_memory[0].memory_type,
      physical_memory[0].smbios_memory_type,
    ),
  };

  Ok(memory_info)
}

///
/// ## MemoryTypeの値に対応するメモリの種類を文字列で返す
///
/// - [TODO] DDR5に対応する
///
fn get_memory_type_description(memory_type: Option<u16>) -> String {
  log_info!(
    &format!("mem type: {:?}", memory_type),
    "get_memory_type_description",
    None::<&str>
  );

  match memory_type {
    Some(0) => "Unknown or Unsupported".to_string(),
    Some(1) => "Other".to_string(),
    Some(2) => "DRAM".to_string(),
    Some(3) => "Synchronous DRAM".to_string(),
    Some(4) => "Cache DRAM".to_string(),
    Some(5) => "EDO".to_string(),
    Some(6) => "EDRAM".to_string(),
    Some(7) => "VRAM".to_string(),
    Some(8) => "SRAM".to_string(),
    Some(9) => "RAM".to_string(),
    Some(10) => "ROM".to_string(),
    Some(11) => "Flash".to_string(),
    Some(12) => "EEPROM".to_string(),
    Some(13) => "FEPROM".to_string(),
    Some(14) => "EPROM".to_string(),
    Some(15) => "CDRAM".to_string(),
    Some(16) => "3DRAM".to_string(),
    Some(17) => "SDRAM".to_string(),
    Some(18) => "SGRAM".to_string(),
    Some(19) => "RDRAM".to_string(),
    Some(20) => "DDR".to_string(),
    Some(21) => "DDR2".to_string(),
    Some(22) => "DDR2 FB-DIMM".to_string(),
    Some(24) => "DDR3".to_string(),
    Some(25) => "FBD2".to_string(),
    Some(26) => "DDR4".to_string(),
    Some(mt) => format!("Other or Unknown Memory Type ({})", mt),
    None => "Unknown".to_string(),
  }
}

///
/// ## MemoryType もしくは SMBIOSMemoryType からメモリの種類を取得
///
fn get_memory_type_with_fallback(
  memory_type: Option<u16>,
  smbios_memory_type: Option<u16>,
) -> String {
  match memory_type {
    Some(0) => match smbios_memory_type {
      Some(20) => "DDR".to_string(),
      Some(21) => "DDR2".to_string(),
      Some(24) => "DDR3".to_string(),
      Some(26) => "DDR4".to_string(),
      Some(34) => "DDR5".to_string(),
      Some(mt) => format!("Other SMBIOS Memory Type ({})", mt),
      None => "Unknown".to_string(),
    },
    Some(mt) => get_memory_type_description(Some(mt)),
    None => "Unknown".to_string(),
  }
}

///
/// ## メモリ情報を別スレッドで取得する（WMIを使用）
///
fn get_memory_info_in_thread<T>(query: String) -> Result<Vec<T>, String>
where
  T: DeserializeOwned + std::fmt::Debug + Send + 'static,
{
  let (tx, rx): (
    Sender<Result<Vec<T>, String>>,
    Receiver<Result<Vec<T>, String>>,
  ) = channel();

  // 別スレッドを起動してWMIクエリを実行
  thread::spawn(move || {
    let result = (|| {
      let com_con = COMLibrary::new()
        .map_err(|e| format!("Failed to initialize COM Library: {:?}", e))?;
      let wmi_con = WMIConnection::new(com_con)
        .map_err(|e| format!("Failed to create WMI connection: {:?}", e))?;

      // WMIクエリを実行してメモリ情報を取得
      let results: Vec<T> = wmi_con
        .raw_query(query)
        .map_err(|e| format!("Failed to execute query: {:?}", e))?;

      log_info!(
        &format!("mem info: {:?}", results),
        "get_memory_info_in_thread",
        &format!("query: {}", query)
      );

      Ok(results)
    })();

    // メインスレッドに結果を送信
    if let Err(err) = tx.send(result) {
      log_error!(
        "Failed to send data from thread",
        "get_wmi_data_in_thread",
        Some(err.to_string())
      );
    }
  });

  // メインスレッドで結果を受信
  rx.recv().map_err(|_| "Failed to receive data from thread".to_string())?
}
