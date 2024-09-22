use crate::utils;

///
/// ## バイト数を単位付きの文字列に変換
///
pub fn format_size(bytes: u64, precision: usize) -> String {
  const KILOBYTE: u64 = 1024;
  const MEGABYTE: u64 = KILOBYTE * 1024;
  const GIGABYTE: u64 = MEGABYTE * 1024;

  if bytes >= GIGABYTE {
    format!(
      "{:.precision$} GB",
      utils::num::round(bytes as f64 / GIGABYTE as f64, precision)
    )
  } else if bytes >= MEGABYTE {
    format!(
      "{:.precision$} MB",
      utils::num::round(bytes as f64 / MEGABYTE as f64, precision)
    )
  } else {
    format!("{} bytes", bytes)
  }
}
