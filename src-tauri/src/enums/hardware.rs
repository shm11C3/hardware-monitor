use serde::{Deserialize, Serialize, Serializer};

#[derive(Debug, PartialEq, Eq, Clone, Deserialize)]
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
