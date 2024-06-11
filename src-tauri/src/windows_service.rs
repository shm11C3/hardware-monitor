use windows::{
  Win32::Foundation::BOOL,
  Win32::UI::WindowsAndMessaging::{SystemParametersInfoA, SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS},
};

pub fn get_windows_theme() -> Result<String, windows::core::Error> {
  let mut theme_mode: BOOL = BOOL(0);
  unsafe {
    let result = SystemParametersInfoA(
      SPI_GETCLIENTAREAANIMATION,
      0,
      Some(&mut theme_mode as *mut _ as *mut _),
      SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    );

    if result.is_err() {
      return Err(windows::core::Error::from_win32());
    }
  }
  let theme = if theme_mode.0 == 0 { "Dark Mode" } else { "Light Mode" };
  Ok(theme.to_string())
}
