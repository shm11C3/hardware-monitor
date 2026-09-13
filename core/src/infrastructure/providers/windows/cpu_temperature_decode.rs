use crate::models::CpuPackageThermalStatus;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CpuTemperatureDecodeError {
  ZeroIntelTemperatureTarget,
  ImplausibleIntelTemperatureTarget(u32),
  ImplausibleIntelPackageTemperature { temperature: i32, target: u32 },
  ZeroAmdTemperatureRegister,
  ImplausibleAmdTemperature,
}

pub(crate) fn decode_intel_temperature_target(
  msr_temperature_target: u64,
) -> Result<u32, CpuTemperatureDecodeError> {
  let target = ((msr_temperature_target >> 16) & 0xff) as u32;
  if target == 0 {
    return Err(CpuTemperatureDecodeError::ZeroIntelTemperatureTarget);
  }
  if !(50..=120).contains(&target) {
    return Err(CpuTemperatureDecodeError::ImplausibleIntelTemperatureTarget(target));
  }
  Ok(target)
}

pub(crate) fn decode_intel_package_temperature(
  temperature_target_celsius: u32,
  package_therm_status: u64,
) -> Result<f32, CpuTemperatureDecodeError> {
  if !(50..=120).contains(&temperature_target_celsius) {
    return Err(
      CpuTemperatureDecodeError::ImplausibleIntelTemperatureTarget(
        temperature_target_celsius,
      ),
    );
  }

  let readout_below_target = ((package_therm_status >> 16) & 0x7f) as i32;
  let temperature = temperature_target_celsius as i32 - readout_below_target;
  if temperature <= 0 || temperature > temperature_target_celsius as i32 {
    return Err(
      CpuTemperatureDecodeError::ImplausibleIntelPackageTemperature {
        temperature,
        target: temperature_target_celsius,
      },
    );
  }

  Ok(temperature as f32)
}

/// Decode only the live package status fields from `IA32_PACKAGE_THERM_STATUS`.
/// Sticky/log fields are intentionally ignored so a sampled value describes
/// the current package state rather than accumulated history.
pub(crate) fn decode_intel_package_thermal_status(
  package_therm_status: u64,
) -> CpuPackageThermalStatus {
  CpuPackageThermalStatus {
    thermal_status: (package_therm_status & (1 << 0)) != 0,
    prochot_or_forcepr_asserted: (package_therm_status & (1 << 2)) != 0,
    power_limitation_status: (package_therm_status & (1 << 10)) != 0,
  }
}

pub(crate) fn decode_amd_zen_package_temperature(
  thm_tcon_cur_tmp: u32,
  tctl_offset_celsius: f32,
) -> Result<f32, CpuTemperatureDecodeError> {
  if thm_tcon_cur_tmp == 0 {
    return Err(CpuTemperatureDecodeError::ZeroAmdTemperatureRegister);
  }

  let cur_temp = ((thm_tcon_cur_tmp >> 21) & 0x7ff) as f32;
  let mut temperature = cur_temp * 0.125;
  if ((thm_tcon_cur_tmp >> 19) & 0x1) == 1 {
    temperature -= 49.0;
  }
  temperature -= tctl_offset_celsius;

  if !(-40.0..=120.0).contains(&temperature) {
    return Err(CpuTemperatureDecodeError::ImplausibleAmdTemperature);
  }

  Ok(temperature)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn intel_target_msr(target: u64) -> u64 {
    target << 16
  }

  fn intel_package_status(readout_below_target: u64) -> u64 {
    readout_below_target << 16
  }

  fn amd_temperature_register(cur_temp: u32, range_sel: bool, reserved_bits: u32) -> u32 {
    (cur_temp << 21) | ((range_sel as u32) << 19) | ((reserved_bits & 0x7) << 16)
  }

  #[test]
  fn intel_temperature_target_decodes_bits_23_to_16() {
    assert_eq!(
      decode_intel_temperature_target(intel_target_msr(100)),
      Ok(100)
    );
  }

  #[test]
  fn intel_temperature_target_rejects_zero() {
    assert_eq!(
      decode_intel_temperature_target(intel_target_msr(0)),
      Err(CpuTemperatureDecodeError::ZeroIntelTemperatureTarget)
    );
  }

  #[test]
  fn intel_temperature_target_rejects_implausible_values() {
    assert_eq!(
      decode_intel_temperature_target(intel_target_msr(125)),
      Err(CpuTemperatureDecodeError::ImplausibleIntelTemperatureTarget(125))
    );
  }

  #[test]
  fn intel_package_temperature_subtracts_readout_from_target() {
    assert_eq!(
      decode_intel_package_temperature(100, intel_package_status(42)),
      Ok(58.0)
    );
  }

  #[test]
  fn intel_package_temperature_accepts_zero_readout_as_target_temperature() {
    assert_eq!(
      decode_intel_package_temperature(100, intel_package_status(0)),
      Ok(100.0)
    );
  }

  #[test]
  fn intel_package_temperature_rejects_negative_result() {
    assert_eq!(
      decode_intel_package_temperature(90, intel_package_status(100)),
      Err(
        CpuTemperatureDecodeError::ImplausibleIntelPackageTemperature {
          temperature: -10,
          target: 90
        }
      )
    );
  }

  #[test]
  fn intel_package_thermal_status_decodes_live_bits_and_ignores_logs() {
    let status = (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3) | (1 << 10) | (1 << 11);

    assert_eq!(
      decode_intel_package_thermal_status(status),
      CpuPackageThermalStatus {
        thermal_status: true,
        prochot_or_forcepr_asserted: true,
        power_limitation_status: true,
      }
    );
  }

  #[test]
  fn intel_package_thermal_status_does_not_infer_state_from_logs() {
    let sticky_logs = (1 << 1) | (1 << 3) | (1 << 11);

    assert_eq!(
      decode_intel_package_thermal_status(sticky_logs),
      CpuPackageThermalStatus::default()
    );
  }

  #[test]
  fn amd_zen_temperature_decodes_cur_temp_in_one_eighth_degrees() {
    let register = amd_temperature_register(512, false, 0);

    assert_eq!(decode_amd_zen_package_temperature(register, 0.0), Ok(64.0));
  }

  #[test]
  fn amd_zen_temperature_honors_range_select_subtracting_49_degrees() {
    let register = amd_temperature_register(800, true, 0);

    assert_eq!(decode_amd_zen_package_temperature(register, 0.0), Ok(51.0));
  }

  #[test]
  fn amd_zen_temperature_applies_tctl_offset() {
    let register = amd_temperature_register(800, false, 0);

    assert_eq!(decode_amd_zen_package_temperature(register, 20.0), Ok(80.0));
  }

  #[test]
  fn amd_zen_temperature_ignores_reserved_tj_sel_bits() {
    let register = amd_temperature_register(800, false, 0b011);

    assert_eq!(decode_amd_zen_package_temperature(register, 0.0), Ok(100.0));
  }

  #[test]
  fn amd_zen_temperature_rejects_all_zero_register() {
    assert_eq!(
      decode_amd_zen_package_temperature(0, 0.0),
      Err(CpuTemperatureDecodeError::ZeroAmdTemperatureRegister)
    );
  }
}
