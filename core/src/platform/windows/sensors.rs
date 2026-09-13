use std::sync::atomic::{AtomicBool, Ordering};

use crate::infrastructure::providers::windows::cpu_temperature::{
  CpuPackageTemperature, CpuPackageTemperatureError, CpuTemperatureSource,
};
use crate::infrastructure::providers::windows::super_io_motherboard::MotherboardSensorReadout;
use crate::log_warn;
use crate::models::{
  ExternalComponentGuidanceCandidate, MotherboardSensorCollection, PowerDraw,
  SensorAvailability, SensorSupport, SensorTemperature, TemperatureSample,
};

static MOTHERBOARD_SENSOR_FALLBACK_LOGGED: AtomicBool = AtomicBool::new(false);

pub fn sample_motherboard_sensors() -> MotherboardSensorCollection {
  let readout =
    crate::infrastructure::providers::windows::super_io_motherboard::sample_motherboard_sensors();
  motherboard_collection_from_readout(readout)
}

/// Convert a provider readout without deriving support from its current rows.
fn motherboard_collection_from_readout(
  readout: MotherboardSensorReadout,
) -> MotherboardSensorCollection {
  let fan_support = readout.fan_support;
  match readout.sample {
    Ok(sample) => MotherboardSensorCollection {
      fan_support,
      sample,
      availability: SensorAvailability::Available,
      guidance_candidates: Vec::new(),
    },
    Err(reason) => {
      if !MOTHERBOARD_SENSOR_FALLBACK_LOGGED.swap(true, Ordering::Relaxed) {
        log_warn!(
          "motherboard_sensor_sampling_failed",
          "platform::windows::sensors::sample_motherboard_sensors",
          Some(reason.clone())
        );
      }

      let availability =
        if reason
          == crate::infrastructure::providers::windows::super_io_motherboard::UNSUPPORTED_SUPER_IO_HM_PATH_REASON
        {
          SensorAvailability::unsupported(reason.clone())
        } else {
          SensorAvailability::unavailable(reason.clone())
        };

      MotherboardSensorCollection {
        sample: Default::default(),
        availability,
        fan_support: if reason
          == crate::infrastructure::providers::windows::super_io_motherboard::UNSUPPORTED_SUPER_IO_HM_PATH_REASON
        {
          SensorSupport::Unsupported
        } else {
          fan_support
        },
        guidance_candidates: motherboard_guidance_candidates_for_reason(reason),
      }
    }
  }
}

fn motherboard_guidance_candidates_for_reason(
  reason: String,
) -> Vec<ExternalComponentGuidanceCandidate> {
  if reason
    == crate::infrastructure::providers::windows::super_io_motherboard::UNSUPPORTED_SUPER_IO_HM_PATH_REASON
    || reason.starts_with(
      crate::infrastructure::providers::windows::super_io_motherboard::ITE_EXPERIMENTAL_NON_COMPONENT_FAILURE_PREFIX,
    )
  {
    return Vec::new();
  }

  vec![ExternalComponentGuidanceCandidate::pawnio_motherboard_sensors(reason)]
}

/// Read the latest CPU / sensor temperatures.
///
/// Windows: prefer CPU package temperature from a recognized PawnIO source,
/// then fall back to ACPI thermal zones via the WMI sampler thread. When only
/// ACPI is available, the headline CPU value picks a CPU-named zone when one
/// exists, otherwise the hottest zone.
pub fn sample_temperatures() -> TemperatureSample {
  use crate::infrastructure::providers::windows::{cpu_temperature, thermal_zone};

  thermal_zone::init_thermal_zone_sampler();
  let sensor_temperatures = thermal_zone::read_thermal_zones_cached();
  build_temperature_sample(
    cpu_temperature::sample_cpu_package_temperature(),
    sensor_temperatures,
  )
}

/// Read the Windows CPU package power path while leaving non-CPU fields
/// unavailable. The RAPL package domain is exposed as `cpu_watts`; the
/// product's derived `package_watts` total is intentionally not populated by
/// this CPU-only source.
pub fn sample_power_draw() -> PowerDraw {
  build_power_draw(
    crate::infrastructure::providers::windows::cpu_power::sample_cpu_package_power(),
  )
}

/// Whether CPU package power has a hardware path, independently of whether
/// the current sample produced watts (the first delta sample intentionally
/// does not).
pub fn cpu_power_support() -> SensorSupport {
  let diagnostics =
    crate::infrastructure::providers::windows::cpu_power::cpu_power_diagnostics();
  if diagnostics.selected_enablement.is_some() {
    SensorSupport::Supported
  } else {
    SensorSupport::Unsupported
  }
}

fn build_power_draw(cpu_watts: Option<f32>) -> PowerDraw {
  PowerDraw {
    cpu_watts,
    ..PowerDraw::default()
  }
}

fn build_temperature_sample(
  pawnio_cpu_temperature: Result<CpuPackageTemperature, CpuPackageTemperatureError>,
  mut sensor_temperatures: Vec<SensorTemperature>,
) -> TemperatureSample {
  match pawnio_cpu_temperature {
    Ok(sample) => {
      sensor_temperatures.insert(
        0,
        SensorTemperature {
          name: cpu_package_sensor_name(&sample).to_string(),
          temperature: sample.temperature_celsius,
        },
      );
      TemperatureSample {
        cpu_temperature: Some(sample.temperature_celsius),
        sensor_temperatures,
        cpu_package_thermal_status: sample.thermal_status,
        availability: SensorAvailability::Available,
        guidance_candidates: Vec::new(),
      }
    }
    Err(pawnio_error) => {
      let cpu_package_thermal_status = match &pawnio_error {
        CpuPackageTemperatureError::Unavailable { thermal_status, .. } => *thermal_status,
        _ => None,
      };
      let pawnio_reason = pawnio_error.to_string();
      let component_failed = matches!(
        &pawnio_error,
        CpuPackageTemperatureError::Unavailable { .. }
      );
      let unsupported =
        matches!(&pawnio_error, CpuPackageTemperatureError::Unsupported(_));
      let cpu_temperature =
        crate::utils::thermal::select_cpu_temperature(&sensor_temperatures);
      let availability = if cpu_temperature.is_none() {
        let reason = match &pawnio_error {
          CpuPackageTemperatureError::Unsupported(_) => format!(
            "PawnIO CPU package path unsupported ({pawnio_reason}); ACPI thermal zones unavailable"
          ),
          CpuPackageTemperatureError::Unavailable { .. } => {
            format!(
              "PawnIO unavailable ({pawnio_reason}); ACPI thermal zones unavailable"
            )
          }
          CpuPackageTemperatureError::Internal(_) => format!(
            "CPU temperature sampler internal error ({pawnio_reason}); ACPI thermal zones unavailable"
          ),
        };
        if unsupported {
          SensorAvailability::unsupported(reason)
        } else {
          SensorAvailability::unavailable(reason)
        }
      } else {
        SensorAvailability::Available
      };
      let guidance_candidates = if component_failed && cpu_temperature.is_none() {
        vec![
          ExternalComponentGuidanceCandidate::pawnio_cpu_package_temperature(
            pawnio_reason,
          ),
        ]
      } else {
        Vec::new()
      };
      TemperatureSample {
        cpu_temperature,
        sensor_temperatures,
        cpu_package_thermal_status,
        availability,
        guidance_candidates,
      }
    }
  }
}

fn cpu_package_sensor_name(sample: &CpuPackageTemperature) -> &'static str {
  match sample.source {
    CpuTemperatureSource::IntelDtsPackageMsr => "CPU Package (PawnIO Intel DTS)",
    CpuTemperatureSource::AmdZenSmnTctl => "CPU Package (PawnIO AMD SMN)",
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::models::CpuPackageThermalStatus;

  #[test]
  fn power_draw_maps_only_cpu_watts() {
    assert_eq!(
      build_power_draw(Some(42.5)),
      PowerDraw {
        cpu_watts: Some(42.5),
        gpu_watts: None,
        ane_watts: None,
        package_watts: None,
      }
    );
  }

  #[test]
  fn power_draw_preserves_an_unavailable_cpu_value() {
    assert_eq!(build_power_draw(None), PowerDraw::default());
  }

  #[test]
  fn motherboard_guidance_suppresses_unsupported_super_io_path() {
    let candidates = motherboard_guidance_candidates_for_reason(
      crate::infrastructure::providers::windows::super_io_motherboard::UNSUPPORTED_SUPER_IO_HM_PATH_REASON
        .to_string(),
    );

    assert!(candidates.is_empty());
  }

  #[test]
  fn motherboard_guidance_remains_for_pawnio_access_failure() {
    let candidates = motherboard_guidance_candidates_for_reason(
      "pawnio_open failed: 0x80070005".to_string(),
    );

    assert_eq!(candidates.len(), 1);
    assert_eq!(
      candidates[0].key,
      crate::models::external_component_guidance::PAWNIO_MOTHERBOARD_SENSORS_KEY
    );
    assert_eq!(
      candidates[0].reason_kind,
      crate::models::ExternalComponentReasonKind::Permission
    );
  }

  #[test]
  fn motherboard_guidance_suppresses_experimental_ite_hardware_state_failure() {
    let candidates = motherboard_guidance_candidates_for_reason(format!(
      "{}: no eligible physical TMPIN channels are enabled",
      crate::infrastructure::providers::windows::super_io_motherboard::ITE_EXPERIMENTAL_NON_COMPONENT_FAILURE_PREFIX
    ));

    assert!(candidates.is_empty());
  }

  #[test]
  fn motherboard_guidance_remains_for_experimental_ite_runtime_failure() {
    let candidates = motherboard_guidance_candidates_for_reason(format!(
      "{}: EC port authorization probe failed: pawnio_execute ioctl_pio_inb failed",
      crate::infrastructure::providers::windows::super_io_motherboard::ITE_EXPERIMENTAL_FAILURE_PREFIX
    ));

    assert_eq!(candidates.len(), 1);
    assert_eq!(
      candidates[0].key,
      crate::models::external_component_guidance::PAWNIO_MOTHERBOARD_SENSORS_KEY
    );
  }

  #[test]
  fn empty_fan_sample_preserves_explicit_provider_support() {
    let collection = motherboard_collection_from_readout(MotherboardSensorReadout {
      sample: Ok(Default::default()),
      fan_support: SensorSupport::Supported,
    });

    assert!(collection.sample.fan_speeds.is_empty());
    assert_eq!(collection.fan_support, SensorSupport::Supported);
  }

  #[test]
  fn failed_sample_preserves_detected_provider_support() {
    let collection = motherboard_collection_from_readout(MotherboardSensorReadout {
      sample: Err("temporary fan read failure".to_string()),
      fan_support: SensorSupport::Supported,
    });

    assert!(collection.sample.fan_speeds.is_empty());
    assert_eq!(collection.fan_support, SensorSupport::Supported);
  }

  #[test]
  fn temperature_sample_prefers_pawnio_cpu_package() {
    let sample = build_temperature_sample(
      Ok(
        crate::infrastructure::providers::windows::cpu_temperature::CpuPackageTemperature {
          temperature_celsius: 61.25,
          source: crate::infrastructure::providers::windows::cpu_temperature::CpuTemperatureSource::IntelDtsPackageMsr,
          thermal_status: Some(crate::models::CpuPackageThermalStatus {
            thermal_status: true,
            prochot_or_forcepr_asserted: false,
            power_limitation_status: true,
          }),
        },
      ),
      vec![SensorTemperature {
        name: "TZ00".into(),
        temperature: 45.0,
      }],
    );

    assert_eq!(sample.cpu_temperature, Some(61.25));
    assert_eq!(
      sample.cpu_package_thermal_status,
      Some(crate::models::CpuPackageThermalStatus {
        thermal_status: true,
        prochot_or_forcepr_asserted: false,
        power_limitation_status: true,
      })
    );
    assert_eq!(
      sample.sensor_temperatures[0].name,
      "CPU Package (PawnIO Intel DTS)"
    );
    assert_eq!(sample.sensor_temperatures[0].temperature, 61.25);
    assert_eq!(sample.availability, SensorAvailability::Available);
  }

  #[test]
  fn temperature_sample_keeps_the_standard_amd_source_label() {
    let sample = build_temperature_sample(
      Ok(CpuPackageTemperature {
        temperature_celsius: 63.5,
        source: CpuTemperatureSource::AmdZenSmnTctl,
        thermal_status: None,
      }),
      Vec::new(),
    );

    assert_eq!(sample.cpu_temperature, Some(63.5));
    assert_eq!(sample.availability, SensorAvailability::Available);
    assert_eq!(
      sample.sensor_temperatures[0].name,
      "CPU Package (PawnIO AMD SMN)"
    );
    assert!(sample.guidance_candidates.is_empty());
  }

  #[test]
  fn temperature_sample_falls_back_to_acpi_cpu_zone() {
    let sample = build_temperature_sample(
      Err(CpuPackageTemperatureError::Unavailable {
        reason: "PawnIOLib.dll not found".to_string(),
        enablement: crate::models::SensorEnablement::Verified,
        thermal_status: None,
      }),
      vec![
        SensorTemperature {
          name: "TZ00".into(),
          temperature: 70.0,
        },
        SensorTemperature {
          name: "CPUZ".into(),
          temperature: 51.0,
        },
      ],
    );

    assert_eq!(sample.cpu_temperature, Some(51.0));
    assert_eq!(sample.availability, SensorAvailability::Available);
  }

  #[test]
  fn temperature_sample_returns_unavailable_reason_when_all_sources_fail() {
    let sample = build_temperature_sample(
      Err(CpuPackageTemperatureError::Unavailable {
        reason: "pawnio_open failed".to_string(),
        enablement: crate::models::SensorEnablement::Verified,
        thermal_status: None,
      }),
      Vec::new(),
    );

    assert_eq!(sample.cpu_temperature, None);
    assert_eq!(
      sample.availability,
      SensorAvailability::unavailable(
        "PawnIO unavailable (pawnio_open failed); ACPI thermal zones unavailable"
      )
    );
  }

  #[test]
  fn temperature_sample_preserves_package_status_when_temperature_decode_fails() {
    let thermal_status = CpuPackageThermalStatus {
      thermal_status: true,
      prochot_or_forcepr_asserted: true,
      power_limitation_status: true,
    };
    let sample = build_temperature_sample(
      Err(CpuPackageTemperatureError::Unavailable {
        reason: "CPU temperature decode failed".to_string(),
        enablement: crate::models::SensorEnablement::Verified,
        thermal_status: Some(thermal_status),
      }),
      Vec::new(),
    );

    assert_eq!(sample.cpu_temperature, None);
    assert_eq!(sample.cpu_package_thermal_status, Some(thermal_status));
  }

  #[test]
  fn temperature_sample_returns_guidance_when_pawnio_and_acpi_fail() {
    let sample = build_temperature_sample(
      Err(CpuPackageTemperatureError::Unavailable {
        reason: "PawnIOLib.dll not found".to_string(),
        enablement: crate::models::SensorEnablement::Verified,
        thermal_status: None,
      }),
      Vec::new(),
    );

    assert_eq!(sample.cpu_temperature, None);
    assert_eq!(sample.guidance_candidates.len(), 1);
    assert_eq!(
      sample.guidance_candidates[0].key,
      "pawnio:cpu-package-temperature:v1"
    );
    assert_eq!(
      sample.guidance_candidates[0].missing_signals,
      vec!["cpu-temperature".to_string()]
    );
  }

  #[test]
  fn temperature_sample_identifies_an_experimental_failure_when_all_sources_fail() {
    let sample = build_temperature_sample(
      Err(CpuPackageTemperatureError::Unavailable {
        reason: "CPU temperature decode failed".to_string(),
        enablement: crate::models::SensorEnablement::Experimental,
        thermal_status: None,
      }),
      Vec::new(),
    );

    assert_eq!(
      sample.availability,
      SensorAvailability::unavailable(
        "PawnIO unavailable (experimental CPU package temperature attempt failed: CPU temperature decode failed); ACPI thermal zones unavailable"
      )
    );
    assert_eq!(
      sample.guidance_candidates[0].diagnostic_detail.as_deref(),
      Some(
        "experimental CPU package temperature attempt failed: CPU temperature decode failed"
      )
    );
  }

  #[test]
  fn temperature_sample_suppresses_guidance_for_unsupported_cpu_path() {
    let sample = build_temperature_sample(
      Err(CpuPackageTemperatureError::Unsupported(
        crate::infrastructure::providers::windows::cpu_temperature::CpuTemperatureFallbackReason::AmdFamilyUnsupported(0x16),
      )),
      Vec::new(),
    );

    assert_eq!(sample.cpu_temperature, None);
    assert!(matches!(
      sample.availability,
      SensorAvailability::Unsupported { .. }
    ));
    assert!(sample.guidance_candidates.is_empty());
  }

  #[test]
  fn temperature_sample_suppresses_guidance_for_internal_failure() {
    let sample = build_temperature_sample(
      Err(CpuPackageTemperatureError::Internal(
        "CPU temperature sampler lock poisoned".to_string(),
      )),
      Vec::new(),
    );

    assert_eq!(sample.cpu_temperature, None);
    assert_eq!(
      sample.availability,
      SensorAvailability::unavailable(
        "CPU temperature sampler internal error (CPU temperature sampler lock poisoned); ACPI thermal zones unavailable"
      )
    );
    assert!(sample.guidance_candidates.is_empty());
  }

  #[test]
  fn temperature_sample_does_not_return_guidance_when_acpi_fallback_succeeds() {
    let sample = build_temperature_sample(
      Err(CpuPackageTemperatureError::Unavailable {
        reason: "PawnIOLib.dll not found".to_string(),
        enablement: crate::models::SensorEnablement::Verified,
        thermal_status: None,
      }),
      vec![SensorTemperature {
        name: "CPUZ".into(),
        temperature: 51.0,
      }],
    );

    assert_eq!(sample.cpu_temperature, Some(51.0));
    assert!(sample.guidance_candidates.is_empty());
  }
}
