#[path = "./windows_service.rs"]
mod windows_service;

use tauri::command;
use windows_service::get_windows_theme;

#[command]
pub fn get_windows_theme_mode() -> String {
  match get_windows_theme() {
    Ok(theme) => theme,
    Err(_) => "Unknown".to_string(),
  }
}
