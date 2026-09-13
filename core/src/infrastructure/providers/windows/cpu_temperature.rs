use std::fmt;
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

pub use super::cpu_identity::{CpuIdentity, CpuVendor};
use super::cpu_identity::{cpuid_leaf, detect_cpu_identity};
use super::cpu_temperature_decode::{
  CpuTemperatureDecodeError, decode_amd_zen_package_temperature,
  decode_intel_package_temperature, decode_intel_package_thermal_status,
  decode_intel_temperature_target,
};
use super::pawn_io::{
  ACCESS_PCI_MUTEX, NamedMutex, PawnIoClient, PawnIoDiscovery, PawnIoInitError,
  PawnIoModule, open_shared_intel_msr,
};
use crate::models::{CpuPackageThermalStatus, SensorEnablement};
use crate::{log_debug, log_warn};

const PAWNIO_MUTEX_TIMEOUT: Duration = Duration::from_millis(50);
const MSR_TEMPERATURE_TARGET: u64 = 0x1a2;
const IA32_PACKAGE_THERM_STATUS: u64 = 0x1b1;
const AMD_THM_TCON_CUR_TMP: u64 = 0x0005_9800;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntelThermalCapabilities {
  pub digital_temperature_sensor: bool,
  pub package_thermal_management: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuTemperatureSource {
  IntelDtsPackageMsr,
  AmdZenSmnTctl,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CpuTemperatureFallbackReason {
  UnsupportedCpuVendor(String),
  IntelDtsUnavailable,
  IntelPackageThermalUnavailable,
  AmdFamilyUnsupported(u32),
}

impl fmt::Display for CpuTemperatureFallbackReason {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::UnsupportedCpuVendor(vendor) => {
        write!(f, "unsupported CPU vendor {vendor}")
      }
      Self::IntelDtsUnavailable => write!(f, "Intel digital thermal sensor unavailable"),
      Self::IntelPackageThermalUnavailable => {
        write!(f, "Intel package thermal management unavailable")
      }
      Self::AmdFamilyUnsupported(family) => {
        write!(
          f,
          "AMD family 0x{family:x} is unsupported by the PawnIO RyzenSMU path"
        )
      }
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CpuPackageTemperatureError {
  Unsupported(CpuTemperatureFallbackReason),
  Unavailable {
    reason: String,
    enablement: SensorEnablement,
    thermal_status: Option<CpuPackageThermalStatus>,
  },
  Internal(String),
}

impl fmt::Display for CpuPackageTemperatureError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Unsupported(reason) => fmt::Display::fmt(reason, f),
      Self::Unavailable {
        reason,
        enablement: SensorEnablement::Experimental,
      } => write!(
        f,
        "experimental CPU package temperature attempt failed: {reason}"
      ),
      Self::Unavailable { reason, .. } | Self::Internal(reason) => f.write_str(reason),
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CpuTemperatureCandidate {
  pub(crate) source: CpuTemperatureSource,
  pub(crate) module: PawnIoModule,
  pub(crate) enablement: SensorEnablement,
}

pub fn amd_family_enablement(family: u32) -> SensorEnablement {
  match family {
    0x17 | 0x19 => SensorEnablement::Verified,
    0x1a => SensorEnablement::Experimental,
    _ => SensorEnablement::Unsupported,
  }
}

pub(crate) fn select_cpu_temperature_candidate(
  cpu: &CpuIdentity,
  intel: Option<IntelThermalCapabilities>,
) -> Result<CpuTemperatureCandidate, CpuTemperatureFallbackReason> {
  match cpu.vendor {
    CpuVendor::Intel => {
      let capabilities = intel.unwrap_or(IntelThermalCapabilities {
        digital_temperature_sensor: false,
        package_thermal_management: false,
      });
      if !capabilities.digital_temperature_sensor {
        return Err(CpuTemperatureFallbackReason::IntelDtsUnavailable);
      }
      if !capabilities.package_thermal_management {
        return Err(CpuTemperatureFallbackReason::IntelPackageThermalUnavailable);
      }
      Ok(CpuTemperatureCandidate {
        source: CpuTemperatureSource::IntelDtsPackageMsr,
        module: PawnIoModule::IntelMsr,
        enablement: SensorEnablement::Verified,
      })
    }
    CpuVendor::Amd => {
      let enablement = amd_family_enablement(cpu.family);
      match enablement {
        SensorEnablement::Verified | SensorEnablement::Experimental => {
          Ok(CpuTemperatureCandidate {
            source: CpuTemperatureSource::AmdZenSmnTctl,
            module: PawnIoModule::RyzenSmu,
            enablement,
          })
        }
        SensorEnablement::Unsupported => Err(
          CpuTemperatureFallbackReason::AmdFamilyUnsupported(cpu.family),
        ),
      }
    }
    CpuVendor::Other => Err(CpuTemperatureFallbackReason::UnsupportedCpuVendor(
      cpu.vendor_id.clone(),
    )),
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuTemperatureDiagnostics {
  pub cpu: CpuIdentity,
  pub intel_capabilities: Option<IntelThermalCapabilities>,
  pub amd_enablement: Option<SensorEnablement>,
  pub pawnio: PawnIoDiscovery,
  pub selected_source: Option<CpuTemperatureSource>,
  pub selected_enablement: Option<SensorEnablement>,
  pub fallback_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CpuPackageTemperature {
  pub temperature_celsius: f32,
  pub source: CpuTemperatureSource,
  pub thermal_status: Option<CpuPackageThermalStatus>,
}

enum ActiveCpuTemperatureSource {
  Intel {
    client: Arc<Mutex<PawnIoClient>>,
    target_celsius: u32,
    enablement: SensorEnablement,
  },
  Amd {
    client: PawnIoClient,
    tctl_offset_celsius: f32,
    enablement: SensorEnablement,
  },
}

struct CpuTemperatureSampler {
  diagnostics: CpuTemperatureDiagnostics,
  active: Option<ActiveCpuTemperatureSource>,
  inactive_error: Option<CpuPackageTemperatureError>,
  sample_failure_logged: bool,
}

static CPU_TEMPERATURE_SAMPLER: OnceLock<Mutex<CpuTemperatureSampler>> = OnceLock::new();

pub fn sample_cpu_package_temperature()
-> Result<CpuPackageTemperature, CpuPackageTemperatureError> {
  let sampler = CPU_TEMPERATURE_SAMPLER.get_or_init(|| {
    let sampler = CpuTemperatureSampler::new();
    log_debug!(
      "cpu_temperature_diagnostics",
      "windows::cpu_temperature::sample_cpu_package_temperature",
      Some(format!("{:?}", sampler.diagnostics))
    );
    Mutex::new(sampler)
  });

  sampler
    .lock()
    .map_err(|_| {
      CpuPackageTemperatureError::Internal(
        "CPU temperature sampler lock poisoned".to_string(),
      )
    })?
    .sample()
}

pub fn cpu_temperature_diagnostics() -> CpuTemperatureDiagnostics {
  let sampler = CPU_TEMPERATURE_SAMPLER.get_or_init(|| {
    let sampler = CpuTemperatureSampler::new();
    log_debug!(
      "cpu_temperature_diagnostics",
      "windows::cpu_temperature::cpu_temperature_diagnostics",
      Some(format!("{:?}", sampler.diagnostics))
    );
    Mutex::new(sampler)
  });

  sampler
    .lock()
    .map(|sampler| sampler.diagnostics.clone())
    .unwrap_or_else(|_| CpuTemperatureDiagnostics::unavailable("sampler lock poisoned"))
}

impl CpuTemperatureDiagnostics {
  fn unavailable(reason: impl Into<String>) -> Self {
    Self {
      cpu: CpuIdentity::unknown(),
      intel_capabilities: None,
      amd_enablement: None,
      pawnio: PawnIoDiscovery::unavailable("sampler unavailable"),
      selected_source: None,
      selected_enablement: None,
      fallback_reason: Some(reason.into()),
    }
  }
}

impl CpuTemperatureSampler {
  fn new() -> Self {
    let cpu = detect_cpu_identity();
    let intel_capabilities = (cpu.vendor == CpuVendor::Intel)
      .then(detect_intel_thermal_capabilities)
      .flatten();
    let amd_enablement =
      (cpu.vendor == CpuVendor::Amd).then(|| amd_family_enablement(cpu.family));

    let candidate = match select_cpu_temperature_candidate(&cpu, intel_capabilities) {
      Ok(candidate) => candidate,
      Err(reason) => {
        let fallback_reason = reason.to_string();
        let mut pawnio = PawnIoDiscovery::probe_install();
        if pawnio.fallback_reason.is_none() {
          pawnio.fallback_reason =
            Some("PawnIO module not selected for unsupported CPU".to_string());
        }
        return Self {
          diagnostics: CpuTemperatureDiagnostics {
            cpu,
            intel_capabilities,
            amd_enablement,
            pawnio,
            selected_source: None,
            selected_enablement: None,
            fallback_reason: Some(fallback_reason),
          },
          active: None,
          inactive_error: Some(CpuPackageTemperatureError::Unsupported(reason)),
          sample_failure_logged: false,
        };
      }
    };

    match ActiveCpuTemperatureSource::open(&cpu, &candidate) {
      Ok((active, pawnio)) => Self {
        diagnostics: CpuTemperatureDiagnostics {
          cpu,
          intel_capabilities,
          amd_enablement,
          pawnio,
          selected_source: Some(candidate.source),
          selected_enablement: Some(candidate.enablement),
          fallback_reason: None,
        },
        active: Some(active),
        inactive_error: None,
        sample_failure_logged: false,
      },
      Err(error) => {
        let PawnIoInitError { discovery, reason } = error;
        Self {
          diagnostics: CpuTemperatureDiagnostics {
            cpu,
            intel_capabilities,
            amd_enablement,
            pawnio: *discovery,
            selected_source: None,
            selected_enablement: Some(candidate.enablement),
            fallback_reason: Some(reason.clone()),
          },
          active: None,
          inactive_error: Some(CpuPackageTemperatureError::Unavailable {
            reason,
            enablement: candidate.enablement,
            thermal_status: None,
          }),
          sample_failure_logged: false,
        }
      }
    }
  }

  fn sample(&mut self) -> Result<CpuPackageTemperature, CpuPackageTemperatureError> {
    let enablement = self
      .active
      .as_ref()
      .map(ActiveCpuTemperatureSource::enablement)
      .unwrap_or(SensorEnablement::Verified);
    let result = match self.active.as_mut() {
      Some(ActiveCpuTemperatureSource::Intel {
        client,
        target_celsius,
        enablement: _,
      }) => read_shared_msr(client, IA32_PACKAGE_THERM_STATUS)
        .map_err(|reason| CpuPackageTemperatureError::Unavailable {
          reason,
          enablement,
          thermal_status: None,
        })
        .and_then(|status| {
          decode_intel_package_temperature_sample(*target_celsius, status, enablement)
        }),
      Some(ActiveCpuTemperatureSource::Amd {
        client,
        tctl_offset_celsius,
        enablement: _,
      }) => match NamedMutex::acquire(ACCESS_PCI_MUTEX, PAWNIO_MUTEX_TIMEOUT) {
        Ok(_mutex) => client
          .read_smu_register(AMD_THM_TCON_CUR_TMP)
          .and_then(|value| {
            decode_amd_zen_package_temperature(value as u32, *tctl_offset_celsius)
              .map_err(format_decode_error)
          })
          .map(|temperature| CpuPackageTemperature {
            temperature_celsius: temperature,
            source: CpuTemperatureSource::AmdZenSmnTctl,
            thermal_status: None,
          }),
        Err(reason) => Err(reason),
      }
      .map_err(|reason| CpuPackageTemperatureError::Unavailable {
        reason,
        enablement,
        thermal_status: None,
      }),
      None => Err(self.inactive_error.clone().unwrap_or_else(|| {
        CpuPackageTemperatureError::Internal(
          "CPU package temperature unavailable".to_string(),
        )
      })),
    };

    if let Err(reason) = &result
      && !self.sample_failure_logged
    {
      self.sample_failure_logged = true;
      log_warn!(
        "cpu_temperature_sample_failed",
        "windows::cpu_temperature::CpuTemperatureSampler::sample",
        Some(reason.to_string())
      );
    }

    result
  }
}

impl ActiveCpuTemperatureSource {
  const fn enablement(&self) -> SensorEnablement {
    match self {
      Self::Intel { enablement, .. } | Self::Amd { enablement, .. } => *enablement,
    }
  }

  fn open(
    cpu: &CpuIdentity,
    candidate: &CpuTemperatureCandidate,
  ) -> Result<(Self, PawnIoDiscovery), PawnIoInitError> {
    match candidate.source {
      CpuTemperatureSource::IntelDtsPackageMsr => {
        let (client, mut discovery) = open_shared_intel_msr()?;
        let target_msr = match read_shared_msr(&client, MSR_TEMPERATURE_TARGET) {
          Ok(value) => value,
          Err(reason) => {
            discovery.fallback_reason = Some(reason.clone());
            return Err(PawnIoInitError {
              discovery: Box::new(discovery),
              reason,
            });
          }
        };
        let target_celsius = match decode_intel_temperature_target(target_msr) {
          Ok(value) => value,
          Err(error) => {
            let reason = format_decode_error(error);
            discovery.fallback_reason = Some(reason.clone());
            return Err(PawnIoInitError {
              discovery: Box::new(discovery),
              reason,
            });
          }
        };
        Ok((
          Self::Intel {
            client,
            target_celsius,
            enablement: candidate.enablement,
          },
          discovery,
        ))
      }
      CpuTemperatureSource::AmdZenSmnTctl => {
        let (client, discovery) = PawnIoClient::open(candidate.module.clone())?;
        Ok((
          Self::Amd {
            client,
            tctl_offset_celsius: amd_tctl_offset_celsius(&cpu.brand),
            enablement: candidate.enablement,
          },
          discovery,
        ))
      }
    }
  }
}

fn read_shared_msr(client: &Arc<Mutex<PawnIoClient>>, msr: u64) -> Result<u64, String> {
  client
    .lock()
    .map_err(|_| "shared IntelMSR client lock poisoned".to_string())?
    .read_msr(msr)
}

fn decode_intel_package_temperature_sample(
  target_celsius: u32,
  package_therm_status: u64,
  enablement: SensorEnablement,
) -> Result<CpuPackageTemperature, CpuPackageTemperatureError> {
  let thermal_status = Some(decode_intel_package_thermal_status(package_therm_status));
  decode_intel_package_temperature(target_celsius, package_therm_status)
    .map(|temperature| CpuPackageTemperature {
      temperature_celsius: temperature,
      source: CpuTemperatureSource::IntelDtsPackageMsr,
      thermal_status,
    })
    .map_err(|error| CpuPackageTemperatureError::Unavailable {
      reason: format_decode_error(error),
      enablement,
      thermal_status,
    })
}

fn format_decode_error(error: CpuTemperatureDecodeError) -> String {
  format!("CPU temperature decode failed: {error:?}")
}

fn detect_intel_thermal_capabilities() -> Option<IntelThermalCapabilities> {
  cpuid_leaf(6).map(|leaf| IntelThermalCapabilities {
    digital_temperature_sensor: (leaf.eax & 0x1) != 0,
    package_thermal_management: (leaf.eax & (1 << 6)) != 0,
  })
}

fn amd_tctl_offset_celsius(brand: &str) -> f32 {
  let brand = brand.to_ascii_uppercase();
  if brand.contains("RYZEN 7 1700X") || brand.contains("RYZEN 7 1800X") {
    20.0
  } else if brand.contains("RYZEN 7 2700X") {
    10.0
  } else {
    0.0
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn cpu(vendor: CpuVendor, vendor_id: &str, family: u32) -> CpuIdentity {
    CpuIdentity {
      vendor,
      vendor_id: vendor_id.to_string(),
      brand: String::new(),
      family,
      model: 0,
    }
  }

  #[test]
  fn amd_family_17h_and_19h_are_verified() {
    assert_eq!(amd_family_enablement(0x17), SensorEnablement::Verified);
    assert_eq!(amd_family_enablement(0x19), SensorEnablement::Verified);
  }

  #[test]
  fn amd_family_1ah_is_experimental() {
    assert_eq!(amd_family_enablement(0x1a), SensorEnablement::Experimental);
  }

  #[test]
  fn intel_candidate_requires_dts_and_package_thermal_management() {
    let cpu = cpu(CpuVendor::Intel, "GenuineIntel", 6);
    assert_eq!(
      select_cpu_temperature_candidate(
        &cpu,
        Some(IntelThermalCapabilities {
          digital_temperature_sensor: true,
          package_thermal_management: true,
        }),
      ),
      Ok(CpuTemperatureCandidate {
        source: CpuTemperatureSource::IntelDtsPackageMsr,
        module: PawnIoModule::IntelMsr,
        enablement: SensorEnablement::Verified,
      })
    );

    assert_eq!(
      select_cpu_temperature_candidate(
        &cpu,
        Some(IntelThermalCapabilities {
          digital_temperature_sensor: true,
          package_thermal_management: false,
        }),
      ),
      Err(CpuTemperatureFallbackReason::IntelPackageThermalUnavailable)
    );
  }

  #[test]
  fn amd_candidate_classifies_verified_experimental_and_unsupported_families() {
    assert_eq!(
      select_cpu_temperature_candidate(&cpu(CpuVendor::Amd, "AuthenticAMD", 0x19), None),
      Ok(CpuTemperatureCandidate {
        source: CpuTemperatureSource::AmdZenSmnTctl,
        module: PawnIoModule::RyzenSmu,
        enablement: SensorEnablement::Verified,
      })
    );

    assert_eq!(
      select_cpu_temperature_candidate(&cpu(CpuVendor::Amd, "AuthenticAMD", 0x1a), None),
      Ok(CpuTemperatureCandidate {
        source: CpuTemperatureSource::AmdZenSmnTctl,
        module: PawnIoModule::RyzenSmu,
        enablement: SensorEnablement::Experimental,
      })
    );
    assert_eq!(
      select_cpu_temperature_candidate(&cpu(CpuVendor::Amd, "AuthenticAMD", 0x16), None),
      Err(CpuTemperatureFallbackReason::AmdFamilyUnsupported(0x16))
    );
  }

  #[test]
  fn experimental_failure_is_identified_only_in_the_error() {
    let error = CpuPackageTemperatureError::Unavailable {
      reason: "CPU temperature decode failed".to_string(),
      enablement: SensorEnablement::Experimental,
      thermal_status: None,
    };

    assert_eq!(
      error.to_string(),
      "experimental CPU package temperature attempt failed: CPU temperature decode failed"
    );

    let verified_error = CpuPackageTemperatureError::Unavailable {
      reason: "CPU temperature decode failed".to_string(),
      enablement: SensorEnablement::Verified,
      thermal_status: None,
    };
    assert_eq!(verified_error.to_string(), "CPU temperature decode failed");
  }

  #[test]
  fn intel_temperature_decode_failure_preserves_live_thermal_status() {
    let package_therm_status = (127 << 16) | (1 << 0) | (1 << 2) | (1 << 10);
    let error = decode_intel_package_temperature_sample(
      100,
      package_therm_status,
      SensorEnablement::Verified,
    )
    .expect_err("the temperature readout should be invalid");

    assert_eq!(
      error,
      CpuPackageTemperatureError::Unavailable {
        reason: "CPU temperature decode failed: ImplausibleIntelPackageTemperature { temperature: -27, target: 100 }".to_string(),
        enablement: SensorEnablement::Verified,
        thermal_status: Some(CpuPackageThermalStatus {
          thermal_status: true,
          prochot_or_forcepr_asserted: true,
          power_limitation_status: true,
        }),
      }
    );
  }

  #[test]
  fn amd_tctl_offset_is_limited_to_ready_phase1_models() {
    assert_eq!(
      amd_tctl_offset_celsius("AMD Ryzen 7 1700X Eight-Core"),
      20.0
    );
    assert_eq!(
      amd_tctl_offset_celsius("AMD Ryzen 7 1800X Eight-Core"),
      20.0
    );
    assert_eq!(
      amd_tctl_offset_celsius("AMD Ryzen 7 2700X Eight-Core"),
      10.0
    );
    assert_eq!(amd_tctl_offset_celsius("AMD Ryzen 9 3900X"), 0.0);
  }
}
