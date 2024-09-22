use nvapi::Kibibytes;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use std::fmt;

pub struct RoundedKibibytes {
  pub kibibytes: Kibibytes,
  pub precision: usize,
}

impl fmt::Display for RoundedKibibytes {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    let value = self.kibibytes.0; // Kibibytesの内部値を取得
    if value < 1000 {
      write!(f, "{} KiB", value)
    } else if value < 1000000 {
      let value_in_mib = value as f32 / 1024.0;
      write!(
        f,
        "{:.precision$} MiB",
        value_in_mib,
        precision = self.precision
      ) // 指定された桁数でフォーマット
    } else {
      let value_in_gib = value as f32 / 1048576.0;
      write!(
        f,
        "{:.precision$} GiB",
        value_in_gib,
        precision = self.precision
      ) // 指定された桁数でフォーマット
    }
  }
}

pub fn round(num: f64, precision: usize) -> f64 {
  Decimal::from_f64(num)
    .map(|d| d.round_dp(precision as u32).to_f64().unwrap_or(0.0))
    .unwrap_or(0.0)
}
