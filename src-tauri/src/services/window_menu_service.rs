use crate::{log_debug, log_error, log_info, log_internal, log_warn};
use tauri::{CustomMenuItem, Menu, MenuItem, Submenu, WindowMenuEvent};

pub fn create_setting() -> Menu {
  let settings = CustomMenuItem::new("preference".to_string(), "Preference");

  let submenu = Submenu::new(
    "File",
    Menu::new().add_item(settings).add_native_item(MenuItem::Quit),
  );

  Menu::new().add_submenu(submenu)
}

pub fn handle_menu_event(event: WindowMenuEvent) {
  match event.menu_item_id() {
    "preference" => {
      log_debug!("preference", "preference", None::<&str>);

      let window = event.window();
      window.emit("open_settings", {}).unwrap();
    }
    _ => {}
  }
}
