use chrono::Local;
use std::{fs, path::PathBuf};
use tracing_subscriber::EnvFilter;

pub fn init(log_dir: PathBuf) {
  if !log_dir.exists() {
    fs::create_dir_all(&log_dir).expect("failed to create log directory");
  }

  // ログファイル名を作成
  let now = Local::now();
  let formatted_date = now.format("%Y-%m-%d_%H-%M-%S").to_string();
  let log_filename = format!("app_log_{}.log", formatted_date);

  let mut log_file = log_dir.clone();
  log_file.push(log_filename);

  let env_filter = if cfg!(debug_assertions) {
    // 開発環境 (デバッグビルド) の場合
    EnvFilter::new("info")
  } else {
    // リリース環境の場合
    EnvFilter::new("warn")
  };

  let log = std::sync::Arc::new(std::fs::File::create(log_file).unwrap());
  tracing_subscriber::fmt()
    .with_ansi(false)
    .with_writer(log)
    .with_env_filter(env_filter)
    .init();
}

#[macro_export]
macro_rules! log_internal {
    ($level:ident, $action:expr, $function_name:expr, $custom_message:expr) => {
        match $custom_message {
            Some(msg) => {
                tracing::$level!(
                    message = %msg,
                    function = %$function_name,
                    action = %$action,
                    "{} - {}",
                    $action,
                    msg
                )
            }
            None => tracing::$level!(
                function = %$function_name,
                action = %$action,
                "{}",
                $action
            ),
        }
    };
}

#[macro_export]
macro_rules! log_debug {
  ($action:expr, $function_name:expr, $custom_message:expr) => {
    log_internal!(debug, $action, $function_name, $custom_message);
  };
}

#[macro_export]
macro_rules! log_info {
  ($action:expr, $function_name:expr, $custom_message:expr) => {
    log_internal!(info, $action, $function_name, $custom_message);
  };
}

#[macro_export]
macro_rules! log_warn {
  ($action:expr, $function_name:expr, $custom_message:expr) => {
    log_internal!(warn, $action, $function_name, $custom_message);
  };
}

#[macro_export]
macro_rules! log_error {
  ($action:expr, $function_name:expr, $custom_message:expr) => {
    log_internal!(error, $action, $function_name, $custom_message);
  };
}
