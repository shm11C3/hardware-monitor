// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Re-export the logging macros from `hardviz_core` so existing
// `use crate::{log_internal, log_warn};` sites keep compiling. The macros
// themselves expand to `tracing::*` calls and live in `hardviz_core::utils::logger`.
pub use hardviz_core::{log_debug, log_error, log_info, log_internal, log_warn};

mod adapters;
mod app;
pub mod cli;
mod commands;
mod enums;
mod infrastructure;
mod lifecycle;
mod models;
mod services;
mod tray;
mod utils;
mod webview_memory;
mod workers;

#[cfg(test)]
mod _tests;

use commands::ambient_sensor;
use commands::background_image;
use commands::cooling_insight;
use commands::external_component_guidance;
use commands::external_component_setup;
use commands::hardware;
use commands::settings;
use commands::system;
use commands::ui;
use commands::updater::app_updates;
use hardviz_core::collector::HistoryStore;
use services::external_component_guidance_service::ExternalComponentGuidanceState;
use std::sync::{Arc, Mutex};
use tauri::Manager;
use tauri::Wry;
use tauri_plugin_autostart::MacosLauncher;
use tauri_specta::{Builder, collect_commands, collect_events};

#[cfg(debug_assertions)]
use specta_typescript::Typescript;

#[cfg(debug_assertions)]
const TYPED_ERROR_IMPL: &str = r#"async function typedError<T, E>(result: Promise<T>): Promise<{ status: "ok"; data: T } | { status: "error"; error: E }> {
    void _assertTypedErrorFollowsContract;
    try {
        return { status: "ok", data: await result };
    } catch (e) {
        return { status: "error", error: e as E };
    }
}
// @ts-expect-error tauri-specta's generated contract assertion leaves E unused under noUnusedLocals.
"#;

/// Apply pending schema migrations against Core's pool, synchronously.
///
/// Runs on a short-lived current-thread runtime because this executes
/// during `run()` setup, before the Tauri (and its Tokio) runtime starts —
/// the same pattern the DB preflight uses. [`db::init`] must have been
/// called first so Core can resolve the database file.
fn apply_pending_migrations() -> Result<(), String> {
  let migrations = infrastructure::database::migration::get_migrations();
  let runtime = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .map_err(|e| format!("Failed to build runtime for migrations: {e}"))?;
  runtime.block_on(hardviz_core::infrastructure::database::migrate::run(
    migrations,
  ))
}

fn build_specta_builder() -> Builder<Wry> {
  Builder::<Wry>::new()
    .events(collect_events![models::hardware::HardwareMonitorUpdate,])
    .commands(collect_commands![
      app_updates::fetch_update,
      app_updates::install_update,
      hardware::get_process_list,
      hardware::get_cpu_usage,
      hardware::get_hardware_info,
      hardware::get_memory_info_detail,
      hardware::get_memory_usage,
      hardware::get_gpu_usage,
      hardware::get_processors_usage,
      hardware::get_gpu_temperature,
      hardware::get_cpu_usage_history,
      hardware::get_memory_usage_history,
      hardware::get_gpu_usage_history,
      hardware::get_network_info,
      hardware::get_super_io_chip_id_diagnostics,
      hardware::get_gpu_memory_usage,
      hardware::get_storage_health_latest_records,
      hardware::get_live_storage_health,
      hardware::refresh_storage_devices,
      external_component_guidance::get_external_component_guidance_candidates,
      external_component_guidance::defer_external_component_guidance_for_session,
      external_component_setup::get_external_component_setup_components,
      external_component_setup::get_external_component_setup_status,
      external_component_setup::run_external_component_setup,
      hardware::get_data_archive_series,
      hardware::get_gpu_archive_series,
      hardware::get_fan_archive_series,
      hardware::get_ambient_archive_series,
      hardware::get_process_stats,
      hardware::get_process_stats_in_period,
      hardware::get_gpu_archive_names,
      cooling_insight::get_cooling_trend,
      cooling_insight::get_cooling_fan_trend,
      cooling_insight::get_cooling_band_comparison,
      cooling_insight::get_cooling_baseline_delta,
      cooling_insight::get_cooling_load_temperature_explorer,
      cooling_insight::get_cooling_covariate_comparison,
      settings::commands::get_settings,
      settings::commands::set_language,
      settings::commands::set_theme,
      settings::commands::set_navigation_layout,
      settings::commands::acknowledge_navigation_restructure_announcement,
      settings::commands::set_display_targets,
      settings::commands::set_power_display_targets,
      settings::commands::set_graph_size,
      settings::commands::set_graph_fit_to_window,
      settings::commands::set_graph_margin_px,
      settings::commands::set_line_graph_type,
      settings::commands::set_line_graph_border,
      settings::commands::set_line_graph_fill,
      settings::commands::set_line_graph_color,
      settings::commands::set_line_graph_mix,
      settings::commands::set_line_graph_show_legend,
      settings::commands::set_line_graph_show_scale,
      settings::commands::set_line_graph_show_tooltip,
      settings::commands::set_background_img_opacity,
      settings::commands::set_selected_background_img,
      settings::commands::set_transparent_ui,
      settings::commands::set_window_opacity,
      settings::commands::set_glass_blur,
      settings::commands::set_temperature_unit,
      settings::commands::set_hardware_archive_enabled,
      settings::commands::set_switchbot_meter_enabled,
      settings::commands::set_switchbot_meter_device,
      ambient_sensor::get_ambient_sensor_candidates,
      settings::commands::set_hardware_archive_retention_days,
      settings::commands::set_hardware_archive_scheduled_data_deletion,
      settings::commands::set_storage_health_retention_days,
      settings::commands::set_burn_in_shift,
      settings::commands::set_burn_in_shift_mode,
      settings::commands::set_burn_in_shift_preset,
      settings::commands::set_burn_in_shift_idle_only,
      settings::commands::set_burn_in_shift_options,
      settings::commands::set_text_selectable,
      settings::commands::set_tray_widget_settings,
      settings::commands::set_close_to_tray_preference,
      settings::commands::acknowledge_external_component_guidance_key,
      settings::commands::set_elevated_startup_mode,
      settings::commands::read_license_file,
      settings::commands::read_third_party_notices_file,
      settings::commands::open_license_file_path,
      background_image::get_background_image,
      background_image::get_background_images,
      background_image::save_background_image,
      background_image::delete_background_image,
      ui::set_decoration,
      system::restart_app,
      system::is_process_elevated,
      system::quit_app,
      system::is_close_to_tray_available,
      system::mark_close_to_tray_listener_ready,
      system::hide_main_window_to_tray,
    ])
}

#[cfg(debug_assertions)]
fn bindings_path() -> std::path::PathBuf {
  std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/rspc/bindings.ts")
}

#[cfg(debug_assertions)]
fn export_typescript_bindings(builder: &Builder<Wry>) {
  builder
    .export(Typescript::default(), bindings_path())
    .expect("Failed to export typescript bindings");
}

/// Build the ambient sensor registry the archive worker reads (#2044).
///
/// The registry is built once and read-only afterwards (#2043), so this
/// is the single point where a user's ambient hardware becomes part of
/// the archive. Returning an empty registry is the normal result: the
/// SwitchBot scan is off by default, and an empty registry makes the
/// archive behave exactly as it did before ambient data existed.
///
/// Never fails and never blocks. Whether a radio exists is discovered
/// asynchronously inside the scan, and a machine without one is not an
/// error worth a dialog - it simply produces no readings, which #2043
/// already reports as an unavailable source.
#[cfg(target_os = "windows")]
fn setup_environmental_sensors(
  app: &tauri::AppHandle,
  core_settings: &hardviz_core::settings::CoreSettings,
  runtime: &tokio::runtime::Handle,
) -> Arc<
  hardviz_core::infrastructure::providers::environmental::EnvironmentalSensorRegistry,
> {
  use hardviz_core::infrastructure::providers::environmental::EnvironmentalSensorRegistry;
  use hardviz_core::infrastructure::providers::switchbot_meter::{
    SwitchBotMeterProvider, SwitchBotScanController,
  };

  let mut registry = EnvironmentalSensorRegistry::new();

  if core_settings.environmental_sensors.switchbot_meter_enabled {
    // Hand the provider the device this machine was told to use. Until
    // the user picks one the provider reports nothing: several sensors
    // in one room can read degrees apart, so choosing for them would be
    // guessing at the number every Thermal Delta is measured against.
    let provider = Arc::new(SwitchBotMeterProvider::new(
      core_settings
        .environmental_sensors
        .chosen_device()
        .map(str::to_string),
    ));

    let scan = SwitchBotScanController::setup(runtime.clone(), Arc::clone(&provider));
    app
      .state::<workers::WorkersState>()
      .switchbot_scan
      .lock()
      .unwrap()
      .replace(scan);
    // Kept beside the registry so the settings screen can list what the
    // radio is hearing right now, which is the only way a user can tell
    // their sensors apart.
    app
      .state::<workers::WorkersState>()
      .switchbot_provider
      .lock()
      .unwrap()
      .replace(Arc::clone(&provider));
    registry.register(provider);
  }

  Arc::new(registry)
}

/// No ambient transport is implemented outside Windows yet (#2044), so
/// the registry is always empty and the archive writes no ambient rows.
///
/// The provider abstraction, the decode, and the cache are all portable;
/// only the radio layer is missing, so adding a platform means adding a
/// scan rather than reworking this.
#[cfg(not(target_os = "windows"))]
fn setup_environmental_sensors(
  _app: &tauri::AppHandle,
  _core_settings: &hardviz_core::settings::CoreSettings,
  _runtime: &tokio::runtime::Handle,
) -> Arc<
  hardviz_core::infrastructure::providers::environmental::EnvironmentalSensorRegistry,
> {
  Arc::default()
}

#[cfg(debug_assertions)]
pub fn export_bindings() {
  let builder = build_specta_builder().typed_error_impl(TYPED_ERROR_IMPL);
  export_typescript_bindings(&builder);
}

/// Run a command-line mode when the process was started with one.
///
/// Returns the exit code to terminate with, or `None` for a normal launch.
/// Called before any Tauri runtime is created so the elevated setup child
/// never competes with the running app for the single-instance lock.
pub fn run_cli_mode_if_requested() -> Option<i32> {
  match cli::parse_cli_mode(std::env::args()) {
    Ok(Some(mode)) => Some(cli::run_cli_mode(mode)),
    Ok(None) => None,
    Err(error) => {
      eprintln!("invalid command line: {error:?}");
      Some(2)
    }
  }
}

pub fn run() {
  let builder = build_specta_builder();

  #[cfg(debug_assertions)]
  let builder = builder.typed_error_impl(TYPED_ERROR_IMPL);

  // TS bindings
  #[cfg(debug_assertions)]
  export_typescript_bindings(&builder);

  let app_state = settings::AppState::new();
  let elevated_startup_mode = app_state.settings.lock().unwrap().elevated_startup_mode;
  let transparent_ui = app_state.settings.lock().unwrap().transparent_ui;
  let glass_blur = app_state.settings.lock().unwrap().glass_blur;

  // Core-owned shared sensor history. App-side commands and the collector
  // loop read/write through this store. Persistence no longer shares it; the
  // archive worker subscribes to the EventBus instead (#1407).
  let history_store = Arc::new(HistoryStore::new());
  let external_component_guidance_state =
    Arc::new(ExternalComponentGuidanceState::default());

  let core_settings = app_state.core_settings.lock().unwrap().clone();

  let db_path = utils::file::get_app_data_dir("hv-database.db");
  // Initialize Core's DB pool location once at process start. Core
  // can't resolve the bundle identifier on its own, so App owns path
  // resolution and hands the file path to
  // `hardviz_core::infrastructure::database::db`. We don't care about
  // the return value here: this is the first and only caller during
  // App startup.
  let _ = hardviz_core::infrastructure::database::db::init(db_path.clone());

  let app_max_version = infrastructure::database::migration::get_max_migration_version();
  let mut db_error = hardviz_core::persistence::preflight::check_db_compatibility(
    &db_path,
    app_max_version,
  );

  // Core owns the database pool, so it also applies the schema migrations —
  // synchronously here, before any persistence worker writes. These were
  // previously registered with `tauri-plugin-sql` but never ran, because
  // the DB is never loaded through the plugin (no `preload`, no frontend
  // `Database.load`), leaving newer tables such as `storage_devices`
  // missing. A migration failure is surfaced as a DB-incompatible startup
  // so the existing recovery dialog handles it.
  if db_error.is_none()
    && let Err(e) = apply_pending_migrations()
  {
    log_error!(
      "Failed to apply database migrations",
      "lib::run",
      Some(e.clone())
    );
    db_error = Some(hardviz_core::persistence::preflight::DbStartupError::Other(
      e,
    ));
  }

  let is_db_ok = db_error.is_none();

  let store_for_setup = Arc::clone(&history_store);
  let guidance_for_setup = Arc::clone(&external_component_guidance_state);

  let tauri_builder = tauri::Builder::<Wry>::default()
    .invoke_handler(builder.invoke_handler())
    .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
      lifecycle::on_second_instance(app);
    }))
    .setup(move |app| {
      let path_resolver = app.path();

      // Initialize logger
      utils::logger::init(path_resolver.app_log_dir().unwrap());

      if elevated_startup_mode {
        match services::system_service::relaunch_for_elevated_startup_if_needed(app.handle())
        {
          Ok(true) => return Ok(()),
          Ok(false) => {}
          Err(e) => {
            log_warn!(
              &format!("Elevated Startup Mode could not restart as administrator: {e}"),
              "lib::setup",
              None::<&str>
            );
          }
        }
      }

      // Initialize UI and real-time monitoring (independent of DB)
      commands::ui::init(app);
      builder.mount_events(app);

      // Apply native macOS vibrancy up front when transparent UI is on and the
      // background-frost toggle (glass_blur) is non-zero, so the frosted glass is
      // composited by the OS instead of the costly CSS backdrop-filter blur
      // (see #1718). No-op on other platforms.
      settings::commands::apply_window_vibrancy(
        app.handle(),
        if transparent_ui { glass_blur } else { 0 },
      );

      // Real-time pipeline: collector publishes MetricsSnapshot to the
      // EventBus, WindowAdapter subscribes and emits HardwareMonitorUpdate.
      let bus = hardviz_core::event_bus::EventBus::new();
      let window_adapter =
        adapters::window::WindowAdapter::setup(app.handle().clone(), bus.subscribe());

      // Run the Core collector on Tauri's tokio runtime. Core has no
      // `tauri` dep, so it can't reach Tauri's static runtime directly —
      // we hand it the inner `tokio::runtime::Handle`.
      let runtime_handle = tauri::async_runtime::handle().inner().clone();
      let monitor = hardviz_core::collector::SystemMonitorController::setup(
        Arc::clone(&store_for_setup),
        bus.clone(),
        runtime_handle.clone(),
      );
      {
        let ws = app.state::<workers::WorkersState>();
        ws.monitor.lock().unwrap().replace(monitor);
        ws.window_adapter.lock().unwrap().replace(window_adapter);
      }

      // #1422: register the tray unconditionally. The visibility
      // policy (always-on vs persisted user setting) is a UX call owned
      // by #1423 and lives outside the adapter.
      match adapters::tray::TrayAdapter::setup(app, bus.subscribe()) {
        Ok(tray) => {
          let ws = app.state::<workers::WorkersState>();
          ws.tray.lock().unwrap().replace(tray);
        }
        Err(e) => {
          app
            .state::<lifecycle::CloseToTrayRuntimeState>()
            .disable_for_session();
          // Linux desktops without an indicator implementation, or any
          // other tray failure: log and continue. Close-to-tray is
          // disabled for this session so the close button still quits.
          log_warn!(
            &format!("tray icon setup failed; continuing without tray: {e}"),
            "lib::setup",
            None::<&str>
          );
        }
      }

      if is_db_ok {
        // Start DB-dependent archive services. Persistence subscribes to
        // the EventBus so a slow DB write can't back-pressure the
        // collector cadence (#1407).
        if core_settings.hardware_archive.enabled {
          // Ambient sources (#2043) ride the archive's one-minute tick,
          // so they are built here and only here: with the archive off
          // there is nowhere for an ambient reading to go, and starting
          // a radio scan to feed a worker that isn't running would be
          // collection cost with no visible value.
          let environmental_sensors = setup_environmental_sensors(
            app.handle(),
            &core_settings,
            &runtime_handle,
          );

          let hw_archive =
            hardviz_core::persistence::ArchiveController::setup_with_environmental_sensors(
              &bus,
              runtime_handle.clone(),
              environmental_sensors,
            );
          {
            let ws = app.state::<workers::WorkersState>();
            ws.hw_archive.lock().unwrap().replace(hw_archive);
          }
        }

        // The cooling daily rollup derives its summary from whatever
        // Hardware Archive rows already exist in the database, so it
        // starts independently of `hardware_archive.enabled`: even when
        // live archive collection is currently turned off, already
        // archived days it hasn't caught up on yet still get rolled up.
        let cooling_rollup_first_catch_up = {
          let (cooling_rollup, first_catch_up) =
            hardviz_core::persistence::CoolingRollupController::setup(runtime_handle.clone());
          let ws = app.state::<workers::WorkersState>();
          ws.cooling_rollup.lock().unwrap().replace(cooling_rollup);
          first_catch_up
        };

        if core_settings.storage_health.enabled {
          match core_settings.storage_health_identity.hash_key_bytes() {
            Ok(identity_hash_key) => {
              let storage_guidance_state = Arc::clone(&guidance_for_setup);
              let storage_guidance_sink:
                hardviz_core::persistence::ExternalComponentGuidanceSink =
                Arc::new(move |candidates| {
                  storage_guidance_state.record_candidates(candidates);
                });
              let storage_health =
                hardviz_core::persistence::StorageHealthController::setup_with_guidance_sink(
                  runtime_handle.clone(),
                  core_settings.storage_health.retention_days,
                  identity_hash_key,
                  Some(storage_guidance_sink),
                );
              // Live Storage Health (ADR 0006): enumerate devices once at
              // startup so on-demand reads never enumerate. The WMI query
              // is blocking, so it runs off the main thread.
              let live_storage_health = Arc::new(
                hardviz_core::collector::LiveStorageHealthCollector::new(
                  identity_hash_key,
                ),
              );
              {
                let ws = app.state::<workers::WorkersState>();
                ws.storage_health.lock().unwrap().replace(storage_health);
                ws.live_storage_health
                  .lock()
                  .unwrap()
                  .replace(Arc::clone(&live_storage_health));
              }
              tauri::async_runtime::spawn_blocking(move || {
                live_storage_health.enumerate_devices()
              });
            }
            Err(e) => {
              log_error!(
                "Storage Health worker was not started because the identity key is invalid",
                "lib::run",
                Some(e)
              );
            }
          }
        }

        // Retention cleanup runs once per process boot — the pre-Phase-4
        // `batch_delete_old_data` wrapper had the same one-shot semantics.
        // The `scheduled_data_deletion` flag still means startup cleanup,
        // not a recurring background schedule.
        // See `hardviz_core::persistence::cleanup_old_data` doc comment.
        //
        // It waits for the cooling rollup's first catch-up pass: archive
        // rows older than the retention cutoff can still be present at
        // startup, and the backfill must read them before they are
        // deleted or those days would be lost from the rollup forever.
        // Cleanup runs only when that pass actually succeeded — after a
        // failed pass (or a dead worker, which closes the channel) this
        // boot's cleanup is skipped so a transient DB error cannot let
        // deletion outrun the rollup; the next boot retries both.
        if core_settings.hardware_archive.scheduled_data_deletion {
          let retention_days = core_settings.hardware_archive.retention_days;
          tauri::async_runtime::spawn(async move {
            if cooling_rollup_first_catch_up.await == Ok(true) {
              hardviz_core::persistence::cleanup_old_data(retention_days).await;
            } else {
              log_warn!(
                "Skipping this boot's retention cleanup: the cooling rollup's first catch-up did not succeed",
                "lib::run",
                None::<&str>
              );
            }
          });
        }
      } else {
        // Database schema is incompatible — show error dialog
        // Hide window while dialog is shown, then restore based on user choice
        if let Some(window) = app.get_webview_window("main") {
          let _ = window.hide();
        }

        let handle = app.handle().clone();
        let db_err = db_error.expect("db_error must be Some when is_db_ok is false");
        std::thread::spawn(move || {
          use app::startup::{self, StartupErrorAction};
          match startup::prompt_startup_error(&handle, db_err) {
            StartupErrorAction::ResetAndRestart => {
              startup::reset_database_and_restart(&handle);
            }
            StartupErrorAction::ContinueAnyway => {
              // Show the main window — app runs without DB-backed features
              if let Some(window) = handle.get_webview_window("main") {
                let _ = window.show();
              }
            }
            StartupErrorAction::Exit => handle.exit(1),
          }
        });
      }

      Ok(())
    })
    .on_window_event(|win, ev| {
      if win.label() == tray::TRAY_WIDGET_FLYOUT_LABEL {
        match ev {
          tauri::WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            if win.hide().is_ok() {
              webview_memory::suspend_for_window(win);
            }
          }
          // An explicit flyout hide also emits Focused(false). Only the
          // focus-loss path for a still-visible flyout owns another hide and
          // suspend transition.
          tauri::WindowEvent::Focused(false)
            if win.is_visible().unwrap_or(false) && win.hide().is_ok() =>
          {
            webview_memory::suspend_for_window(win);
          }
          _ => {}
        }
        return;
      }

      if win.label() == "main"
        && let tauri::WindowEvent::CloseRequested { api, .. } = ev
      {
        // Always prevent the default close so lifecycle picks the
        // outcome (hide vs. quit) deterministically — see
        // `lifecycle::handle_close_request` for the policy.
        api.prevent_close();
        lifecycle::on_close_requested(win);
      }
    })
    .plugin(tauri_plugin_updater::Builder::new().build())
    .plugin(tauri_plugin_store::Builder::new().build())
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_window_state::Builder::default().build())
    .plugin(tauri_plugin_shell::init())
    .plugin(tauri_plugin_autostart::init(
      MacosLauncher::LaunchAgent,
      Some(vec![]),
    ))
    .plugin(tauri_plugin_clipboard_manager::init())
    .plugin(tauri_plugin_os::init())
    .plugin(tauri_plugin_opener::init())
    .manage(history_store)
    .manage(app_state)
    .manage(external_component_guidance_state)
    .manage(lifecycle::CloseToTrayRuntimeState::default())
    .manage(workers::WorkersState::default())
    .manage(app_updates::PendingUpdate(Mutex::new(None)));

  let mut context = tauri::generate_context!();
  utils::tauri::apply_runtime_config(context.config_mut());

  tauri_builder
    .build(context)
    .expect("error while building tauri application")
    .run(lifecycle::on_run_event);
}
