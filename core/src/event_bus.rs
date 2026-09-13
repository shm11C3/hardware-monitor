//! In-process broadcast channel for [`MetricsSnapshot`].
//!
//! `EventBus` lets the collector publish a single snapshot per tick that
//! is fanned out to multiple subscribers (window adapter, persistence,
//! future tray/overlay/alert consumers) without any of them depending on
//! Tauri or on each other.

use tokio::sync::broadcast;

use crate::models::MetricsSnapshot;

/// Default channel capacity. Snapshots are produced at ~1 Hz, so 16 slots
/// give every subscriber ample headroom before they observe a `Lagged`
/// error from `broadcast::Receiver::recv`.
pub const DEFAULT_CAPACITY: usize = 16;

#[derive(Clone)]
pub struct EventBus {
  tx: broadcast::Sender<MetricsSnapshot>,
}

impl EventBus {
  pub fn new() -> Self {
    Self::with_capacity(DEFAULT_CAPACITY)
  }

  /// Create an event bus with the given broadcast channel capacity.
  ///
  /// # Panics
  ///
  /// Panics if `capacity` is zero. (`tokio::sync::broadcast::channel`
  /// itself panics in that case; this assertion just gives a clearer
  /// message at the source-of-truth boundary.)
  pub fn with_capacity(capacity: usize) -> Self {
    assert!(
      capacity > 0,
      "EventBus::with_capacity requires a non-zero capacity"
    );
    let (tx, _rx) = broadcast::channel(capacity);
    Self { tx }
  }

  /// Publish a snapshot to all current subscribers. With no subscribers
  /// the snapshot is silently dropped.
  pub fn publish(&self, snapshot: MetricsSnapshot) {
    let _ = self.tx.send(snapshot);
  }

  /// Create a new subscriber. Subscribers only see snapshots published
  /// after this call returns.
  pub fn subscribe(&self) -> broadcast::Receiver<MetricsSnapshot> {
    self.tx.subscribe()
  }

  /// Number of currently active subscribers. Primarily for tests and
  /// observability.
  pub fn subscriber_count(&self) -> usize {
    self.tx.receiver_count()
  }
}

impl Default for EventBus {
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::models::GpuMetric;

  fn snapshot(cpu_usage: f32) -> MetricsSnapshot {
    MetricsSnapshot {
      cpu_usage,
      memory_usage: 0.0,
      processors_usage: vec![],
      gpus: vec![],
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

  #[tokio::test(flavor = "current_thread")]
  async fn single_subscriber_receives_published_snapshot() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe();
    bus.publish(snapshot(50.0));

    let received = rx.recv().await.expect("receive snapshot");
    assert_eq!(received.cpu_usage, 50.0);
  }

  #[tokio::test(flavor = "current_thread")]
  async fn fan_out_reaches_every_subscriber() {
    let bus = EventBus::new();
    let mut a = bus.subscribe();
    let mut b = bus.subscribe();
    let mut c = bus.subscribe();
    assert_eq!(bus.subscriber_count(), 3);

    bus.publish(snapshot(75.0));

    for rx in [&mut a, &mut b, &mut c] {
      let received = rx.recv().await.expect("receive snapshot");
      assert_eq!(received.cpu_usage, 75.0);
    }
  }

  #[tokio::test(flavor = "current_thread")]
  async fn late_subscriber_does_not_observe_prior_messages() {
    let bus = EventBus::new();
    bus.publish(snapshot(10.0));

    let mut rx = bus.subscribe();
    bus.publish(snapshot(20.0));

    let received = rx.recv().await.expect("receive snapshot");
    assert_eq!(received.cpu_usage, 20.0);
  }

  #[test]
  fn publish_with_no_subscribers_does_not_panic() {
    let bus = EventBus::new();
    bus.publish(snapshot(42.0));
    assert_eq!(bus.subscriber_count(), 0);
  }

  #[test]
  #[should_panic(expected = "non-zero capacity")]
  fn with_capacity_zero_panics() {
    let _ = EventBus::with_capacity(0);
  }

  #[tokio::test(flavor = "current_thread")]
  async fn snapshot_carries_gpu_payload() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe();
    bus.publish(MetricsSnapshot {
      cpu_usage: 0.0,
      memory_usage: 0.0,
      processors_usage: vec![],
      gpus: vec![GpuMetric {
        gpu_id: "test:0".into(),
        gpu_name: "Test GPU".into(),
        gpu_usage: Some(80.0),
        gpu_temperature: Some(70.0),
        gpu_source: "Test".into(),
        gpu_dedicated_memory_usage_kb: Some(2048.0),
        gpu_cooler_level: Some(50),
      }],
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
    });

    let received = rx.recv().await.expect("receive snapshot");
    assert_eq!(received.gpus.len(), 1);
    assert_eq!(received.gpus[0].gpu_id, "test:0");
    assert_eq!(received.gpus[0].gpu_usage, Some(80.0));
  }

  #[tokio::test(flavor = "current_thread")]
  async fn dropping_a_subscriber_decrements_count() {
    let bus = EventBus::new();
    let rx_a = bus.subscribe();
    let _rx_b = bus.subscribe();
    assert_eq!(bus.subscriber_count(), 2);

    drop(rx_a);
    assert_eq!(bus.subscriber_count(), 1);
  }
}
