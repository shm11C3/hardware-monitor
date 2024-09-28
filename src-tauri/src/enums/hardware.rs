use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum HardwareType {
  CPU,
  Memory,
  GPU,
}

impl Serialize for HardwareType {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    let s = match *self {
      HardwareType::CPU => "cpu",
      HardwareType::Memory => "memory",
      HardwareType::GPU => "gpu",
    };
    serializer.serialize_str(s)
  }
}

impl<'de> Deserialize<'de> for HardwareType {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let s = String::deserialize(deserializer)?.to_lowercase();
    match s.as_str() {
      "cpu" => Ok(HardwareType::CPU),
      "memory" => Ok(HardwareType::Memory),
      "gpu" => Ok(HardwareType::GPU),
      _ => Err(serde::de::Error::unknown_variant(
        &s,
        &["cpu", "memory", "gpu"],
      )),
    }
  }
}
