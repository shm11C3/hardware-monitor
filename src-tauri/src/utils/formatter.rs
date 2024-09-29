use crate::utils;
use nvapi::Kibibytes;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use std::fmt;

pub struct RoundedKibibytes {
  pub kibibytes: Kibibytes,
  pub precision: usize,
}

///
/// ## `Kibibytes` をフォーマット
///
impl fmt::Display for RoundedKibibytes {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    let value = self.kibibytes.0; // Kibibytesの内部値を取得
    if value < 1000 {
      write!(f, "{} KB", value)
    } else if value < 1000000 {
      let value_in_mib = value as f32 / 1024.0;
      write!(
        f,
        "{:.precision$} MB",
        value_in_mib,
        precision = self.precision
      ) // 指定された桁数でフォーマット
    } else {
      let value_in_gib = value as f32 / 1048576.0;
      write!(
        f,
        "{:.precision$} GB",
        value_in_gib,
        precision = self.precision
      ) // 指定された桁数でフォーマット
    }
  }
}

///
/// ## 小数を指定桁数で丸める（四捨五入）
///
pub fn round(num: f64, precision: usize) -> f64 {
  Decimal::from_f64(num)
    .map(|d| d.round_dp(precision as u32).to_f64().unwrap_or(0.0))
    .unwrap_or(0.0)
}

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
      round(bytes as f64 / GIGABYTE as f64, precision)
    )
  } else if bytes >= MEGABYTE {
    format!(
      "{:.precision$} MB",
      round(bytes as f64 / MEGABYTE as f64, precision)
    )
  } else {
    format!("{} bytes", bytes)
  }
}

///
/// ## ベンダー名をフォーマット
///
pub fn format_vendor_name(vendor_id: &str) -> String {
  match vendor_id {
    "GenuineIntel" => "Intel".to_string(),
    "AuthenticAMD" => "AMD".to_string(),
    _ => vendor_id.to_string(),
  }
}
