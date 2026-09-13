//! Integration test for #1407.
//!
//! Goal: a slow persistence subscriber must not back-pressure the
//! collector's publish cadence on the EventBus. This is the structural
//! benefit of the Phase-4 split — persistence subscribes to the bus
//! instead of sharing the collector's history bag, so a stalled DB
//! write cannot make the publisher wait.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use hardviz_core::event_bus::EventBus;
use hardviz_core::models::MetricsSnapshot;

fn snapshot(value: f32) -> MetricsSnapshot {
  MetricsSnapshot {
    cpu_usage: value,
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn slow_persistence_subscriber_does_not_block_publisher() {
  let bus = EventBus::new();
  let publisher_ticks = Arc::new(AtomicUsize::new(0));

  // Slow subscriber simulating a persistence task whose DB write takes
  // far longer than the collector's tick. With shared `Arc<Mutex<...>>`
  // state the publisher would have to wait for the subscriber to release
  // the lock; with the broadcast bus, `publish` returns immediately.
  let mut slow_rx = bus.subscribe();
  let slow = tokio::spawn(async move {
    while let Ok(_snap) = slow_rx.recv().await {
      tokio::time::sleep(Duration::from_millis(100)).await;
    }
  });

  // Drive the publisher at 50 Hz for 250 ms — far above the slow
  // subscriber's drain rate. We expect ~12 ticks regardless of the
  // subscriber being stalled.
  let publisher_ticks_clone = Arc::clone(&publisher_ticks);
  let bus_clone = bus.clone();
  let publisher = tokio::spawn(async move {
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(250) {
      bus_clone.publish(snapshot(0.0));
      publisher_ticks_clone.fetch_add(1, Ordering::Relaxed);
      tokio::time::sleep(Duration::from_millis(20)).await;
    }
  });

  publisher.await.unwrap();

  let ticks = publisher_ticks.load(Ordering::Relaxed);
  // At 20 ms per tick over 250 ms we expect ~12 publishes; allow slack
  // for scheduler jitter on busy CI runners. The key signal is that
  // the slow subscriber's 100 ms sleep doesn't cap the publisher.
  assert!(
    ticks >= 8,
    "publisher only ticked {ticks} times — slow subscriber appears to be back-pressuring"
  );

  // Drop the bus so the subscriber's recv loop terminates cleanly.
  drop(bus);
  let _ = slow.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lagged_subscriber_does_not_take_down_publisher() {
  // Capacity of 4 means a subscriber that stalls past 4 messages will
  // observe `Lagged` errors but the publisher must keep going.
  let bus = EventBus::with_capacity(4);

  let mut stuck_rx = bus.subscribe();
  let stuck = tokio::spawn(async move {
    // Sleep past the publisher window before draining anything.
    tokio::time::sleep(Duration::from_millis(150)).await;
    let mut errors = 0;
    let mut received = 0;
    while let Ok(result) =
      tokio::time::timeout(Duration::from_millis(50), stuck_rx.recv()).await
    {
      match result {
        Ok(_) => received += 1,
        Err(_) => {
          errors += 1;
        }
      }
    }
    (received, errors)
  });

  // Publish more than the channel capacity to force the lag condition.
  for i in 0..32 {
    bus.publish(snapshot(i as f32));
    tokio::time::sleep(Duration::from_millis(5)).await;
  }

  let (received, errors) = stuck.await.unwrap();
  // The publisher must not panic regardless of subscriber lag, and a
  // stalled subscriber that catches up should still see at least the
  // tail of the stream once it's draining.
  assert!(
    received > 0 || errors > 0,
    "stuck subscriber should observe either snapshots or lag events"
  );
}
