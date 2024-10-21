use crate::enums::hardware;
use crate::utils::color;
use crate::utils::file::get_app_data_dir;
use crate::{log_debug, log_error, log_info, log_internal, log_warn, utils};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
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
pub struct LineGraphColorSettings {
  pub cpu: [u8; 3],
  pub memory: [u8; 3],
  pub gpu: [u8; 3],
}

///
/// ## settings.json に格納するJSONの構造体
///
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
  version: String,
  language: String,
  theme: String,
  display_targets: Vec<hardware::HardwareType>,
  graph_size: String,
  line_graph_border: bool,
  line_graph_fill: bool,
  line_graph_color: LineGraphColorSettings,
  line_graph_mix: bool,
  line_graph_show_legend: bool,
  line_graph_show_scale: bool,
  state: StateSettings,
}

///
/// クライアントに送信する設定の構造体
///
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LineGraphColorStringSettings {
  pub cpu: String,
  pub memory: String,
  pub gpu: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ClientSettings {
  language: String,
  theme: String,
  display_targets: Vec<hardware::HardwareType>,
  graph_size: String,
  line_graph_border: bool,
  line_graph_fill: bool,
  line_graph_color: LineGraphColorStringSettings,
  line_graph_mix: bool,
  line_graph_show_legend: bool,
  line_graph_show_scale: bool,
  state: StateSettings,
}

impl Default for Settings {
  fn default() -> Self {
    Self {
      version: utils::tauri::get_app_version(&utils::tauri::get_config()),
      language: "en".to_string(),
      theme: "dark".to_string(),
      display_targets: vec![
        hardware::HardwareType::CPU,
        hardware::HardwareType::Memory,
        hardware::HardwareType::GPU,
      ],
      graph_size: "xl".to_string(),
      line_graph_border: true,
      line_graph_fill: true,
      line_graph_color: LineGraphColorSettings {
        cpu: [75, 192, 192],
        memory: [255, 99, 132],
        gpu: [255, 206, 86],
      },
      line_graph_mix: false,
      line_graph_show_legend: true,
      line_graph_show_scale: false,
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

  pub fn set_line_graph_border(&mut self, new_value: bool) -> Result<(), String> {
    self.line_graph_border = new_value;
    self.write_file()
  }

  pub fn set_line_graph_fill(&mut self, new_value: bool) -> Result<(), String> {
    self.line_graph_fill = new_value;
    self.write_file()
  }

  ///
  /// ## グラフの色を設定する
  ///
  /// - グラフの色は #ffffff 形式の文字列で入力される
  /// - グラフの色は RGB 形式の値に変換して保存する
  ///
  pub fn set_line_graph_color(
    &mut self,
    key: hardware::HardwareType,
    new_color: String,
  ) -> Result<String, String> {
    let new_color = match color::hex_to_rgb(&new_color) {
      Ok(rgb) => rgb,
      Err(e) => {
        log_error!("Invalid color format", "set_line_graph_color", Some(e));
        return Err("Invalid color format".to_string());
      }
    };

    match key {
      hardware::HardwareType::CPU => {
        self.line_graph_color.cpu = new_color;
      }
      hardware::HardwareType::Memory => {
        self.line_graph_color.memory = new_color;
      }
      hardware::HardwareType::GPU => {
        self.line_graph_color.gpu = new_color;
      }
    }

    self.write_file();

    match key {
      hardware::HardwareType::CPU => Ok(
        self
          .line_graph_color
          .cpu
          .iter()
          .map(|&c| c.to_string())
          .collect::<Vec<String>>()
          .join(","),
      ),
      hardware::HardwareType::Memory => Ok(
        self
          .line_graph_color
          .memory
          .iter()
          .map(|&c| c.to_string())
          .collect::<Vec<String>>()
          .join(","),
      ),
      hardware::HardwareType::GPU => Ok(
        self
          .line_graph_color
          .gpu
          .iter()
          .map(|&c| c.to_string())
          .collect::<Vec<String>>()
          .join(","),
      ),
    }
  }

  pub fn set_line_graph_mix(&mut self, new_value: bool) -> Result<(), String> {
    self.line_graph_mix = new_value;
    self.write_file()
  }

  pub fn set_line_graph_show_legend(&mut self, new_value: bool) -> Result<(), String> {
    self.line_graph_show_legend = new_value;
    self.write_file()
  }

  pub fn set_line_graph_show_scale(&mut self, new_value: bool) -> Result<(), String> {
    self.line_graph_show_scale = new_value;
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
  use tauri::{Emitter, EventTarget, Window};

  const ERROR_TITLE: &str = "設定の更新に失敗しました";
  const ERROR_MESSAGE: &str = "何度も発生する場合は settings.json を削除してください";

  ///
  /// ## エラーイベントを発生させフロントエンドに通知する
  ///
  /// [TODO] dialog を使ってエラーメッセージを表示する
  ///
  fn emit_error(window: &Window) -> Result<(), String> {
    window
      .emit_to(
        EventTarget::window(window.label().to_string()),
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
  ) -> Result<ClientSettings, String> {
    let settings = state.settings.lock().unwrap().clone();

    // フロントで扱いやすいようにカンマ区切りの文字列に変換する
    let color_strings = LineGraphColorStringSettings {
      cpu: settings
        .line_graph_color
        .cpu
        .iter()
        .map(|&c| c.to_string())
        .collect::<Vec<String>>()
        .join(","),
      memory: settings
        .line_graph_color
        .memory
        .iter()
        .map(|&c| c.to_string())
        .collect::<Vec<String>>()
        .join(","),
      gpu: settings
        .line_graph_color
        .gpu
        .iter()
        .map(|&c| c.to_string())
        .collect::<Vec<String>>()
        .join(","),
    };

    let client_settings = ClientSettings {
      language: settings.language,
      theme: settings.theme,
      display_targets: settings.display_targets,
      graph_size: settings.graph_size,
      line_graph_border: settings.line_graph_border,
      line_graph_fill: settings.line_graph_fill,
      line_graph_color: color_strings,
      line_graph_mix: settings.line_graph_mix,
      line_graph_show_legend: settings.line_graph_show_legend,
      line_graph_show_scale: settings.line_graph_show_scale,
      state: settings.state,
    };

    Ok(client_settings)
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
  pub async fn set_line_graph_border(
    window: Window,
    state: tauri::State<'_, AppState>,
    new_value: bool,
  ) -> Result<(), String> {
    let mut settings = state.settings.lock().unwrap();

    if let Err(e) = settings.set_line_graph_border(new_value) {
      emit_error(&window)?;
      return Err(e);
    }
    Ok(())
  }

  #[tauri::command]
  pub async fn set_line_graph_fill(
    window: Window,
    state: tauri::State<'_, AppState>,
    new_value: bool,
  ) -> Result<(), String> {
    let mut settings = state.settings.lock().unwrap();

    if let Err(e) = settings.set_line_graph_fill(new_value) {
      emit_error(&window)?;
      return Err(e);
    }
    Ok(())
  }

  #[tauri::command]
  pub async fn set_line_graph_color(
    window: Window,
    state: tauri::State<'_, AppState>,
    target: hardware::HardwareType,
    new_color: String,
  ) -> Result<String, String> {
    let mut settings = state.settings.lock().unwrap();

    match settings.set_line_graph_color(target, new_color) {
      Ok(result) => Ok(result),
      Err(e) => {
        emit_error(&window)?;
        Err(e)
      }
    }
  }

  #[tauri::command]
  pub async fn set_line_graph_mix(
    window: Window,
    state: tauri::State<'_, AppState>,
    new_value: bool,
  ) -> Result<(), String> {
    let mut settings = state.settings.lock().unwrap();

    if let Err(e) = settings.set_line_graph_mix(new_value) {
      emit_error(&window)?;
      return Err(e);
    }
    Ok(())
  }

  #[tauri::command]
  pub async fn set_line_graph_show_legend(
    window: Window,
    state: tauri::State<'_, AppState>,
    new_value: bool,
  ) -> Result<(), String> {
    let mut settings = state.settings.lock().unwrap();

    if let Err(e) = settings.set_line_graph_show_legend(new_value) {
      emit_error(&window)?;
      return Err(e);
    }
    Ok(())
  }

  #[tauri::command]
  pub async fn set_line_graph_show_scale(
    window: Window,
    state: tauri::State<'_, AppState>,
    new_value: bool,
  ) -> Result<(), String> {
    let mut settings = state.settings.lock().unwrap();

    if let Err(e) = settings.set_line_graph_show_scale(new_value) {
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
