use crate::enums::hardware;
use crate::utils::file::get_app_data_dir;
use crate::{log_debug, log_error, log_info, log_internal, log_warn};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::mem;
use std::sync::Mutex;

const SETTINGS_FILENAME: &str = "settings.json";

trait Config {
  fn write_file(&self) -> Result<(), String>;
  fn read_file(&mut self) -> Result<(), String>;
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StateSettings {
  pub display: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
  language: String,
  theme: String,
  display_targets: Vec<hardware::HardwareType>,
  graph_size: String,
  state: StateSettings,
}

impl Default for Settings {
  fn default() -> Self {
    Self {
      language: "en".to_string(),
      theme: "dark".to_string(),
      display_targets: vec![
        hardware::HardwareType::CPU,
        hardware::HardwareType::Memory,
        hardware::HardwareType::GPU,
      ],
      graph_size: "xl".to_string(),
      state: StateSettings {
        display: "dashboard".to_string(),
      },
    }
  }
}

impl Config for Settings {
  fn write_file(&self) -> Result<(), String> {
    let config_file = get_app_data_dir(SETTINGS_FILENAME);
    if !config_file.parent().unwrap().exists() {
      fs::create_dir_all(config_file.parent().unwrap()).unwrap();
    }

    match serde_json::to_string(self) {
      Ok(serialized) => {
        if let Err(e) = fs::File::create(config_file)
          .and_then(|mut file| file.write_all(&serialized.as_bytes()))
        {
          // [TODO] ログの定数化
          log_error!(
            "Failed to serialize settings",
            "write_file",
            Some(e.to_string())
          );
          return Err(format!("Failed to write to settings file: {}", e));
        }
      }
      Err(e) => {
        log_error!(
          "Failed to serialize settings",
          "write_file",
          Some(e.to_string())
        );
        return Err(format!("Failed to serialize settings: {}", e));
      }
    }

    Ok(())
  }

  fn read_file(&mut self) -> Result<(), String> {
    let config_file = get_app_data_dir(SETTINGS_FILENAME);

    match fs::read_to_string(config_file) {
      Ok(input) => match serde_json::from_str::<Self>(&input) {
        Ok(deserialized) => {
          *self = deserialized;
          Ok(())
        }
        Err(e) => {
          log_error!(
            "Failed to deserialize settings",
            "read_file",
            Some(e.to_string())
          );
          Err(format!("Failed to deserialize settings: {}", e))
        }
      },
      Err(e) => {
        log_error!(
          "Failed to deserialize settings",
          "read_file",
          Some(e.to_string())
        );
        Err(format!("Failed to read settings file: {}", e))
      }
    }
  }
}

impl Settings {
  pub fn new() -> Self {
    let config_file = get_app_data_dir(SETTINGS_FILENAME);

    let mut settings = Self::default();

    if !config_file.exists() {
      return settings;
    }

    if let Err(e) = settings.read_file() {
      log_error!("read_config_failed", "read_file", Some(e.to_string()));
    }

    settings
  }

  pub fn set_language(&mut self, new_lang: String) -> Result<(), String> {
    self.language = new_lang;
    self.write_file()
  }

  pub fn set_theme(&mut self, new_theme: String) -> Result<(), String> {
    self.theme = new_theme;
    self.write_file()
  }

  pub fn set_display_targets(
    &mut self,
    new_targets: Vec<hardware::HardwareType>,
  ) -> Result<(), String> {
    self.display_targets = new_targets;
    self.write_file()
  }

  pub fn set_graph_size(&mut self, new_size: String) -> Result<(), String> {
    self.graph_size = new_size;
    self.write_file()
  }

  pub fn set_state(&mut self, key: &str, new_value: String) -> Result<(), String> {
    match key {
      "display" => self.state.display = new_value,
      _ => return Err(format!("Invalid key: {}", key)),
    }
    self.write_file()
  }
}

#[derive(Debug)]
pub struct AppState {
  settings: Mutex<Settings>,
}

impl AppState {
  pub fn new() -> Self {
    Self {
      settings: Mutex::from(Settings::new()),
    }
  }
}

pub mod commands {
  use super::*;
  use serde_json::json;
  use tauri::Window;

  const ERROR_TITLE: &str = "設定の更新に失敗しました";
  const ERROR_MESSAGE: &str = "何度も発生する場合は settings.json を削除してください";

  fn emit_error(window: &Window) -> Result<(), String> {
    window
      .emit(
        "error_event",
        json!({
            "title": ERROR_TITLE,
            "message": ERROR_MESSAGE
        }),
      )
      .map_err(|e| format!("Failed to emit event: {}", e))?;

    Ok(())
  }

  #[tauri::command]
  pub async fn get_settings(
    state: tauri::State<'_, AppState>,
  ) -> Result<Settings, String> {
    let settings = state.settings.lock().unwrap().clone();
    Ok(settings)
  }

  #[tauri::command]
  pub async fn set_language(
    window: Window,
    state: tauri::State<'_, AppState>,
    new_language: String,
  ) -> Result<(), String> {
    let mut settings = state.settings.lock().unwrap();

    if let Err(e) = settings.set_language(new_language) {
      emit_error(&window)?;
      return Err(e);
    }

    Ok(())
  }

  #[tauri::command]
  pub async fn set_theme(
    window: Window,
    state: tauri::State<'_, AppState>,
    new_theme: String,
  ) -> Result<(), String> {
    let mut settings = state.settings.lock().unwrap();

    if let Err(e) = settings.set_theme(new_theme) {
      emit_error(&window)?;
      return Err(e);
    }

    Ok(())
  }

  #[tauri::command]
  pub async fn set_display_targets(
    window: Window,
    state: tauri::State<'_, AppState>,
    new_targets: Vec<hardware::HardwareType>,
  ) -> Result<(), String> {
    let mut settings = state.settings.lock().unwrap();

    if let Err(e) = settings.set_display_targets(new_targets) {
      emit_error(&window)?;
      return Err(e);
    }
    Ok(())
  }

  #[tauri::command]
  pub async fn set_graph_size(
    window: Window,
    state: tauri::State<'_, AppState>,
    new_size: String,
  ) -> Result<(), String> {
    let mut settings = state.settings.lock().unwrap();

    if let Err(e) = settings.set_graph_size(new_size) {
      emit_error(&window)?;
      return Err(e);
    }
    Ok(())
  }

  #[tauri::command]
  pub async fn set_state(
    window: Window,
    state: tauri::State<'_, AppState>,
    key: String,
    new_value: String,
  ) -> Result<(), String> {
    let mut settings = state.settings.lock().unwrap();

    if let Err(e) = settings.set_state(&key, new_value) {
      emit_error(&window)?;
      log_error!(
        "Failed to update settings",
        "set_state",
        Some(e.to_string())
      );

      return Err(e);
    }

    Ok(())
  }
}
