///
/// ## バイト数を単位付きの文字列に変換
///
pub fn format_size(bytes: u64) -> String {
  const KILOBYTE: u64 = 1024;
  const MEGABYTE: u64 = KILOBYTE * 1024;
  const GIGABYTE: u64 = MEGABYTE * 1024;

  if bytes >= GIGABYTE {
    format!("{:.2} GB", bytes as f64 / GIGABYTE as f64)
  } else if bytes >= MEGABYTE {
    format!("{:.2} MB", bytes as f64 / MEGABYTE as f64)
  } else {
    format!("{} bytes", bytes)
  }
}
