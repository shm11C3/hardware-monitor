use std::time::Duration;

use hardviz_core::models::MetricsSnapshot;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::enums::settings::TemperatureUnit;

const DEFAULT_UPDATE_INTERVAL_SECS: u64 = 1;
const UPDATE_INTERVALS_SECS: [u64; 3] = [1, 2, 5];

#[cfg(target_os = "macos")]
const CONFIGURABLE_METRICS: [TrayMetric; 2] = [TrayMetric::Cpu, TrayMetric::Gpu];
#[cfg(not(target_os = "macos"))]
const CONFIGURABLE_METRICS: [TrayMetric; 3] =
  [TrayMetric::Cpu, TrayMetric::Gpu, TrayMetric::GpuTemp];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum TrayMetric {
  Cpu,
  Gpu,
  #[serde(alias = "temp")]
  GpuTemp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TrayWidgetSettings {
  #[serde(default)]
  pub enabled: bool,
  #[serde(default)]
  pub metric_order: Vec<TrayMetric>,
  #[serde(default)]
  pub visible_metrics: Vec<TrayMetric>,
  #[serde(default)]
  pub update_interval_secs: u64,
  #[serde(default, rename = "metrics", skip_serializing)]
  legacy_metrics: Vec<TrayMetric>,
}

impl Default for TrayWidgetSettings {
  fn default() -> Self {
    Self {
      enabled: false,
      metric_order: default_metric_vec(),
      visible_metrics: default_metric_vec(),
      update_interval_secs: DEFAULT_UPDATE_INTERVAL_SECS,
      legacy_metrics: vec![],
    }
  }
}

impl TrayWidgetSettings {
  pub fn normalized(mut self) -> Self {
    let legacy_metrics = normalize_metrics(std::mem::take(&mut self.legacy_metrics));

    if self.metric_order.is_empty() && !legacy_metrics.is_empty() {
      self.metric_order = legacy_metrics.clone();
    }
    if self.visible_metrics.is_empty() && !legacy_metrics.is_empty() {
      self.visible_metrics = legacy_metrics;
    }

    self.metric_order = normalize_metric_order(self.metric_order);
    self.visible_metrics =
      normalize_visible_metrics(self.visible_metrics, &self.metric_order);

    if !UPDATE_INTERVALS_SECS.contains(&self.update_interval_secs) {
      self.update_interval_secs = DEFAULT_UPDATE_INTERVAL_SECS;
    }

    self
  }

  pub fn update_interval(&self) -> Duration {
    Duration::from_secs(self.update_interval_secs)
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetricState {
  Normal,
  Warning,
  Critical,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrayMetricSample {
  pub metric: TrayMetric,
  pub value: f32,
  pub state: MetricState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayMetricIcon {
  pub fallback_label: &'static str,
  pub macos_symbol_name: &'static str,
  pub accessibility_label: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayFrameItem {
  pub metric: TrayMetric,
  pub icon: TrayMetricIcon,
  pub value: String,
  pub state: MetricState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayFrame {
  pub title: Option<String>,
  pub items: Vec<TrayFrameItem>,
  pub tooltip: String,
}

pub fn build_frame(
  snapshot: &MetricsSnapshot,
  settings: &TrayWidgetSettings,
  temperature_unit: &TemperatureUnit,
) -> TrayFrame {
  if !settings.enabled {
    return TrayFrame {
      title: None,
      items: vec![],
      tooltip: "HardwareVisualizer".to_string(),
    };
  }

  let samples = collect_samples(snapshot, settings);

  if samples.is_empty() {
    return TrayFrame {
      title: None,
      items: vec![],
      tooltip: "HardwareVisualizer: no tray metrics available".to_string(),
    };
  }

  let items = samples
    .iter()
    .map(|sample| format_frame_item(sample, temperature_unit))
    .collect::<Vec<_>>();
  let title = samples
    .iter()
    .map(|sample| format_title_sample(sample, temperature_unit))
    .collect::<Vec<_>>()
    .join("  ");

  let tooltip = samples
    .iter()
    .map(|sample| format_tooltip_sample(sample, temperature_unit))
    .collect::<Vec<_>>()
    .join("\n");

  TrayFrame {
    title: Some(title),
    items,
    tooltip,
  }
}

pub fn collect_samples(
  snapshot: &MetricsSnapshot,
  settings: &TrayWidgetSettings,
) -> Vec<TrayMetricSample> {
  let visible_metrics = settings
    .visible_metrics
    .iter()
    .copied()
    .collect::<std::collections::HashSet<_>>();

  settings
    .metric_order
    .iter()
    .filter(|metric| visible_metrics.contains(metric))
    .filter_map(|metric| match metric {
      TrayMetric::Cpu => Some((*metric, snapshot.cpu_usage)),
      TrayMetric::Gpu => max_gpu_usage(snapshot).map(|value| (*metric, value)),
      TrayMetric::GpuTemp => {
        hottest_temperature_celsius(snapshot).map(|value| (*metric, value))
      }
    })
    .map(|(metric, value)| TrayMetricSample {
      metric,
      value,
      state: classify_metric_state(metric, value),
    })
    .collect()
}

fn default_metric_vec() -> Vec<TrayMetric> {
  CONFIGURABLE_METRICS.to_vec()
}

fn normalize_metric_order(metrics: Vec<TrayMetric>) -> Vec<TrayMetric> {
  let mut normalized = normalize_metrics(metrics);

  for metric in CONFIGURABLE_METRICS {
    if !normalized.contains(&metric) {
      normalized.push(metric);
    }
  }

  normalized
}

fn normalize_visible_metrics(
  metrics: Vec<TrayMetric>,
  metric_order: &[TrayMetric],
) -> Vec<TrayMetric> {
  let mut visible_metrics = normalize_metrics(metrics)
    .into_iter()
    .filter(|metric| metric_order.contains(metric))
    .collect::<Vec<_>>();

  if visible_metrics.is_empty() {
    visible_metrics = metric_order.to_vec();
  }

  visible_metrics
}

fn normalize_metrics(metrics: Vec<TrayMetric>) -> Vec<TrayMetric> {
  let mut normalized = Vec::with_capacity(metrics.len());

  for metric in metrics {
    if CONFIGURABLE_METRICS.contains(&metric) && !normalized.contains(&metric) {
      normalized.push(metric);
    }
  }

  normalized
}

pub fn classify_metric_state(metric: TrayMetric, value: f32) -> MetricState {
  match metric {
    TrayMetric::Cpu => classify(value, 70.0, 85.0),
    TrayMetric::Gpu => classify(value, 70.0, 90.0),
    TrayMetric::GpuTemp => classify(value, 70.0, 85.0),
  }
}

pub fn should_skip_frame(
  previous: Option<&TrayFrame>,
  next: &TrayFrame,
  elapsed_since_last_update: Option<Duration>,
  update_interval: Duration,
) -> bool {
  if previous.is_some_and(|previous| previous == next) {
    return true;
  }

  elapsed_since_last_update.is_some_and(|elapsed| elapsed < update_interval)
}

fn classify(value: f32, warning_threshold: f32, critical_threshold: f32) -> MetricState {
  if value >= critical_threshold {
    MetricState::Critical
  } else if value >= warning_threshold {
    MetricState::Warning
  } else {
    MetricState::Normal
  }
}

fn max_gpu_usage(snapshot: &MetricsSnapshot) -> Option<f32> {
  snapshot
    .gpus
    .iter()
    .filter_map(|gpu| gpu.gpu_usage)
    .max_by(f32::total_cmp)
}

fn hottest_temperature_celsius(snapshot: &MetricsSnapshot) -> Option<f32> {
  snapshot
    .gpus
    .iter()
    .filter_map(|gpu| gpu.gpu_temperature)
    .max_by(f32::total_cmp)
}

fn format_title_sample(
  sample: &TrayMetricSample,
  temperature_unit: &TemperatureUnit,
) -> String {
  format!(
    "{} {}{}",
    metric_icon(sample.metric),
    format_value(sample.metric, sample.value, temperature_unit),
    state_suffix(sample.state)
  )
}

fn format_frame_item(
  sample: &TrayMetricSample,
  temperature_unit: &TemperatureUnit,
) -> TrayFrameItem {
  TrayFrameItem {
    metric: sample.metric,
    icon: metric_icon_config(sample.metric),
    value: format_value(sample.metric, sample.value, temperature_unit),
    state: sample.state,
  }
}

fn metric_icon(metric: TrayMetric) -> &'static str {
  metric_icon_config(metric).fallback_label
}

fn metric_icon_config(metric: TrayMetric) -> TrayMetricIcon {
  match metric {
    TrayMetric::Cpu => TrayMetricIcon {
      fallback_label: "CPU",
      macos_symbol_name: "cpu",
      accessibility_label: "CPU usage",
    },
    TrayMetric::Gpu => TrayMetricIcon {
      fallback_label: "GPU",
      macos_symbol_name: "display",
      accessibility_label: "GPU usage",
    },
    TrayMetric::GpuTemp => TrayMetricIcon {
      fallback_label: "TEMP",
      macos_symbol_name: "thermometer",
      accessibility_label: "GPU temperature",
    },
  }
}

fn format_tooltip_sample(
  sample: &TrayMetricSample,
  temperature_unit: &TemperatureUnit,
) -> String {
  let label = match sample.metric {
    TrayMetric::Cpu => "CPU usage",
    TrayMetric::Gpu => "GPU usage",
    TrayMetric::GpuTemp => "GPU temp",
  };

  format!(
    "{}{}: {}",
    format_state(sample.state),
    label,
    format_value(sample.metric, sample.value, temperature_unit),
  )
}

fn format_value(
  metric: TrayMetric,
  value: f32,
  temperature_unit: &TemperatureUnit,
) -> String {
  match metric {
    TrayMetric::Cpu | TrayMetric::Gpu => format!("{}%", value.round() as i32),
    TrayMetric::GpuTemp => {
      let (value, suffix) = match temperature_unit {
        TemperatureUnit::Celsius => (value, "°C"),
        TemperatureUnit::Fahrenheit => (value * 9.0 / 5.0 + 32.0, "°F"),
      };
      format!("{}{}", value.round() as i32, suffix)
    }
  }
}

fn state_suffix(state: MetricState) -> &'static str {
  match state {
    MetricState::Normal => "",
    MetricState::Warning => "!",
    MetricState::Critical => "!!",
  }
}

fn format_state(state: MetricState) -> &'static str {
  match state {
    MetricState::Normal => "🟢",
    MetricState::Warning => "🟡",
    MetricState::Critical => "🔴",
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use hardviz_core::models::GpuMetric;

  fn snapshot(cpu_usage: f32, gpus: Vec<GpuMetric>) -> MetricsSnapshot {
    MetricsSnapshot {
      cpu_usage,
      memory_usage: 50.0,
      processors_usage: vec![],
      gpus,
      processes: vec![],
      cpu_temperature: None,
      cpu_package_thermal_status: None,
      sensor_temperatures: vec![],
      motherboard_temperatures: vec![],
      motherboard_fan_speeds: vec![],
      power_draw: Default::default(),
      cpu_power_support: Default::default(),
      motherboard_fan_support: Default::default(),
      external_component_guidance_candidates: vec![],
    }
  }

  fn gpu(usage: Option<f32>, temperature: Option<f32>) -> GpuMetric {
    GpuMetric {
      gpu_id: "gpu:0".to_string(),
      gpu_name: "GPU".to_string(),
      gpu_usage: usage,
      gpu_temperature: temperature,
      gpu_source: "Test".to_string(),
      gpu_dedicated_memory_usage_kb: None,
      gpu_cooler_level: None,
    }
  }

  #[test]
  fn classifies_usage_and_temperature_boundaries() {
    assert_eq!(
      classify_metric_state(TrayMetric::Cpu, 69.9),
      MetricState::Normal
    );
    assert_eq!(
      classify_metric_state(TrayMetric::Cpu, 70.0),
      MetricState::Warning
    );
    assert_eq!(
      classify_metric_state(TrayMetric::Cpu, 85.0),
      MetricState::Critical
    );
    assert_eq!(
      classify_metric_state(TrayMetric::Gpu, 89.9),
      MetricState::Warning
    );
    assert_eq!(
      classify_metric_state(TrayMetric::Gpu, 90.0),
      MetricState::Critical
    );
    assert_eq!(
      classify_metric_state(TrayMetric::GpuTemp, 85.0),
      MetricState::Critical
    );
  }

  #[test]
  fn builds_ordered_frame_with_available_metrics() {
    let settings = TrayWidgetSettings {
      enabled: true,
      metric_order: vec![TrayMetric::Cpu, TrayMetric::Gpu],
      visible_metrics: vec![TrayMetric::Cpu, TrayMetric::Gpu],
      update_interval_secs: 1,
      legacy_metrics: vec![],
    };
    let frame = build_frame(
      &snapshot(
        42.4,
        vec![gpu(Some(18.2), Some(58.1)), gpu(Some(55.8), Some(70.0))],
      ),
      &settings,
      &TemperatureUnit::Celsius,
    );

    assert_eq!(frame.title, Some("CPU 42%  GPU 56%".to_string()));
    assert_eq!(
      frame.items,
      vec![
        TrayFrameItem {
          metric: TrayMetric::Cpu,
          icon: metric_icon_config(TrayMetric::Cpu),
          value: "42%".to_string(),
          state: MetricState::Normal,
        },
        TrayFrameItem {
          metric: TrayMetric::Gpu,
          icon: metric_icon_config(TrayMetric::Gpu),
          value: "56%".to_string(),
          state: MetricState::Normal,
        },
      ]
    );
    assert!(frame.tooltip.contains("🟢CPU usage: 42%"));
  }

  #[test]
  fn builds_gpu_temperature_frame_with_state_emoji_and_temperature_unit() {
    let settings = TrayWidgetSettings {
      enabled: true,
      metric_order: vec![TrayMetric::GpuTemp],
      visible_metrics: vec![TrayMetric::GpuTemp],
      update_interval_secs: 1,
      legacy_metrics: vec![],
    };

    let celsius_frame = build_frame(
      &snapshot(42.4, vec![gpu(None, Some(58.1)), gpu(None, Some(70.0))]),
      &settings,
      &TemperatureUnit::Celsius,
    );

    assert_eq!(celsius_frame.title, Some("TEMP 70°C!".to_string()));
    assert_eq!(celsius_frame.tooltip, "🟡GPU temp: 70°C");
    assert_eq!(
      celsius_frame.items,
      vec![TrayFrameItem {
        metric: TrayMetric::GpuTemp,
        icon: metric_icon_config(TrayMetric::GpuTemp),
        value: "70°C".to_string(),
        state: MetricState::Warning,
      }]
    );

    let fahrenheit_frame = build_frame(
      &snapshot(42.4, vec![gpu(None, Some(85.0))]),
      &settings,
      &TemperatureUnit::Fahrenheit,
    );

    assert_eq!(fahrenheit_frame.title, Some("TEMP 185°F!!".to_string()));
    assert_eq!(fahrenheit_frame.tooltip, "🔴GPU temp: 185°F");
  }

  #[test]
  fn build_frame_includes_platform_icon_metadata_for_visible_metrics() {
    let settings = TrayWidgetSettings {
      enabled: true,
      metric_order: vec![TrayMetric::Cpu, TrayMetric::Gpu],
      visible_metrics: vec![TrayMetric::Cpu, TrayMetric::Gpu],
      update_interval_secs: 1,
      legacy_metrics: vec![],
    };
    let frame = build_frame(
      &snapshot(42.4, vec![gpu(Some(55.8), Some(70.0))]),
      &settings,
      &TemperatureUnit::Celsius,
    );

    let icons = frame
      .items
      .iter()
      .map(|item| {
        (
          item.metric,
          item.icon.fallback_label,
          item.icon.macos_symbol_name,
          item.icon.accessibility_label,
        )
      })
      .collect::<Vec<_>>();

    assert_eq!(
      icons,
      vec![
        (TrayMetric::Cpu, "CPU", "cpu", "CPU usage"),
        (TrayMetric::Gpu, "GPU", "display", "GPU usage"),
      ]
    );
  }

  #[test]
  fn disabled_widget_clears_title() {
    let frame = build_frame(
      &snapshot(90.0, vec![gpu(Some(80.0), Some(90.0))]),
      &TrayWidgetSettings::default(),
      &TemperatureUnit::Celsius,
    );

    assert_eq!(frame.title, None);
    assert_eq!(frame.tooltip, "HardwareVisualizer");
  }

  #[test]
  fn enabled_widget_without_available_metrics_keeps_empty_frame() {
    let settings = TrayWidgetSettings {
      enabled: true,
      metric_order: vec![TrayMetric::Gpu],
      visible_metrics: vec![TrayMetric::Gpu],
      update_interval_secs: 1,
      legacy_metrics: vec![],
    };
    let frame = build_frame(
      &snapshot(42.4, vec![]),
      &settings,
      &TemperatureUnit::Celsius,
    );

    assert_eq!(frame.title, None);
    assert_eq!(frame.items, vec![]);
    assert_eq!(
      frame.tooltip,
      "HardwareVisualizer: no tray metrics available"
    );
  }

  #[test]
  fn skips_identical_frame_before_throttle_check() {
    let frame = TrayFrame {
      title: Some("C 42%".to_string()),
      items: vec![],
      tooltip: "🟢CPU usage: 42%".to_string(),
    };

    assert!(should_skip_frame(
      Some(&frame),
      &frame,
      Some(Duration::from_secs(3)),
      Duration::from_secs(1)
    ));
  }

  #[test]
  fn skips_changed_frame_until_interval_elapses() {
    let previous = TrayFrame {
      title: Some("C 42%".to_string()),
      items: vec![],
      tooltip: "🟢CPU usage: 42%".to_string(),
    };
    let next = TrayFrame {
      title: Some("C 43%".to_string()),
      items: vec![],
      tooltip: "🟢CPU usage: 43%".to_string(),
    };

    assert!(should_skip_frame(
      Some(&previous),
      &next,
      Some(Duration::from_millis(999)),
      Duration::from_secs(1)
    ));
    assert!(!should_skip_frame(
      Some(&previous),
      &next,
      Some(Duration::from_secs(1)),
      Duration::from_secs(1)
    ));
  }

  #[test]
  fn normalizes_empty_metric_order_and_interval() {
    let settings = TrayWidgetSettings {
      enabled: true,
      metric_order: vec![],
      visible_metrics: vec![],
      update_interval_secs: 99,
      legacy_metrics: vec![],
    }
    .normalized();

    assert_eq!(settings.metric_order, CONFIGURABLE_METRICS.to_vec());
    assert_eq!(settings.visible_metrics, CONFIGURABLE_METRICS.to_vec());
    assert_eq!(settings.update_interval_secs, 1);
  }

  #[test]
  fn normalizes_discrete_update_intervals() {
    for interval in [1, 2, 5] {
      let settings = TrayWidgetSettings {
        enabled: true,
        metric_order: vec![],
        visible_metrics: vec![],
        update_interval_secs: interval,
        legacy_metrics: vec![],
      }
      .normalized();

      assert_eq!(settings.update_interval_secs, interval);
    }

    for interval in [0, 3, 4, 99] {
      let settings = TrayWidgetSettings {
        enabled: true,
        metric_order: vec![],
        visible_metrics: vec![],
        update_interval_secs: interval,
        legacy_metrics: vec![],
      }
      .normalized();

      assert_eq!(settings.update_interval_secs, 1);
    }
  }

  #[test]
  fn filters_visible_metrics_by_metric_order() {
    let settings = TrayWidgetSettings {
      enabled: true,
      metric_order: vec![TrayMetric::GpuTemp, TrayMetric::Cpu, TrayMetric::Gpu],
      visible_metrics: vec![TrayMetric::Gpu, TrayMetric::Cpu],
      update_interval_secs: 1,
      legacy_metrics: vec![],
    };
    let frame = build_frame(
      &snapshot(42.4, vec![gpu(Some(18.2), Some(58.1))]),
      &settings,
      &TemperatureUnit::Celsius,
    );

    assert_eq!(frame.title, Some("CPU 42%  GPU 18%".to_string()));
  }

  #[test]
  fn normalizes_legacy_metrics_store_shape() {
    let settings = TrayWidgetSettings {
      enabled: true,
      metric_order: vec![],
      visible_metrics: vec![],
      update_interval_secs: 2,
      legacy_metrics: vec![TrayMetric::GpuTemp, TrayMetric::Cpu],
    }
    .normalized();

    #[cfg(target_os = "macos")]
    {
      assert_eq!(
        settings.metric_order,
        vec![TrayMetric::Cpu, TrayMetric::Gpu]
      );
      assert_eq!(settings.visible_metrics, vec![TrayMetric::Cpu]);
    }
    #[cfg(not(target_os = "macos"))]
    {
      assert_eq!(
        settings.metric_order,
        vec![TrayMetric::GpuTemp, TrayMetric::Cpu, TrayMetric::Gpu]
      );
      assert_eq!(
        settings.visible_metrics,
        vec![TrayMetric::GpuTemp, TrayMetric::Cpu]
      );
    }
  }

  #[test]
  fn deserializes_old_temperature_metric_key() {
    let settings = serde_json::from_value::<TrayWidgetSettings>(serde_json::json!({
      "metricOrder": ["gpu", "temp", "cpu"],
      "visibleMetrics": ["temp", "gpu"]
    }))
    .expect("old tray widget temperature metric key should deserialize")
    .normalized();

    #[cfg(target_os = "macos")]
    {
      assert_eq!(
        settings.metric_order,
        vec![TrayMetric::Gpu, TrayMetric::Cpu]
      );
      assert_eq!(settings.visible_metrics, vec![TrayMetric::Gpu]);
    }
    #[cfg(not(target_os = "macos"))]
    {
      assert_eq!(
        settings.metric_order,
        vec![TrayMetric::Gpu, TrayMetric::GpuTemp, TrayMetric::Cpu]
      );
      assert_eq!(
        settings.visible_metrics,
        vec![TrayMetric::GpuTemp, TrayMetric::Gpu]
      );
    }
  }

  #[test]
  fn deserializes_partial_legacy_store_shape() {
    let settings = serde_json::from_value::<TrayWidgetSettings>(serde_json::json!({
      "metrics": ["gpu", "gpu-temp", "cpu"]
    }))
    .expect("partial legacy tray widget settings should deserialize")
    .normalized();

    assert!(!settings.enabled);
    assert_eq!(settings.update_interval_secs, 1);
    #[cfg(target_os = "macos")]
    {
      assert_eq!(
        settings.metric_order,
        vec![TrayMetric::Gpu, TrayMetric::Cpu]
      );
      assert_eq!(
        settings.visible_metrics,
        vec![TrayMetric::Gpu, TrayMetric::Cpu]
      );
    }
    #[cfg(not(target_os = "macos"))]
    {
      assert_eq!(
        settings.metric_order,
        vec![TrayMetric::Gpu, TrayMetric::GpuTemp, TrayMetric::Cpu]
      );
      assert_eq!(
        settings.visible_metrics,
        vec![TrayMetric::Gpu, TrayMetric::GpuTemp, TrayMetric::Cpu]
      );
    }
  }

  #[test]
  fn keeps_temperature_in_current_store_shape() {
    let settings = TrayWidgetSettings {
      enabled: true,
      metric_order: vec![TrayMetric::GpuTemp, TrayMetric::Gpu],
      visible_metrics: vec![TrayMetric::GpuTemp, TrayMetric::Gpu],
      update_interval_secs: 1,
      legacy_metrics: vec![],
    }
    .normalized();

    #[cfg(target_os = "macos")]
    {
      assert_eq!(
        settings.metric_order,
        vec![TrayMetric::Gpu, TrayMetric::Cpu]
      );
      assert_eq!(settings.visible_metrics, vec![TrayMetric::Gpu]);
    }
    #[cfg(not(target_os = "macos"))]
    {
      assert_eq!(
        settings.metric_order,
        vec![TrayMetric::GpuTemp, TrayMetric::Gpu, TrayMetric::Cpu]
      );
      assert_eq!(
        settings.visible_metrics,
        vec![TrayMetric::GpuTemp, TrayMetric::Gpu]
      );
    }
  }
}
