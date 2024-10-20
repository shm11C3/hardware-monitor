pub fn hex_to_rgb(hex: &str) -> Result<[u8; 3], &str> {
  if hex.len() != 7 || !hex.starts_with('#') {
    return Err("Invalid hex format");
  }
  let r = u8::from_str_radix(&hex[1..3], 16).map_err(|_| "Invalid hex value")?;
  let g = u8::from_str_radix(&hex[3..5], 16).map_err(|_| "Invalid hex value")?;
  let b = u8::from_str_radix(&hex[5..7], 16).map_err(|_| "Invalid hex value")?;
  Ok([r, g, b])
}
