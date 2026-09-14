//! Tauri-independent core for HardwareVisualizer.
//!
//! Module bodies are populated incrementally across the phases of #1402.
//! The Cargo dependency graph enforces "no `tauri::*` under `core/src/`"
//! at compile time.

pub mod collector;

pub mod persistence;

pub mod monitoring;

pub mod event_bus;

pub mod settings;

pub mod platform;

pub mod infrastructure;

pub mod enums;

pub mod models;

/// External Component Setup: pinned catalog and setup execution.
pub mod external_component_setup;

pub mod utils;
