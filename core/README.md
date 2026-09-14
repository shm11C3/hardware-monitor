# hardviz-core

Tauri-independent core crate for HardwareVisualizer.

`hardviz-core` (library name `hardviz_core`) owns sensor collection, persistence,
the in-process event bus, and Core-consumed settings. It is the lower half of
the Core / App split introduced by [#1402][issue-1402]: everything that does
not need a Tauri context lives here, and the Tauri app crate
([`src-tauri/`](../src-tauri)) depends on it via a path dependency.

[issue-1402]: https://github.com/shm11C3/HardwareVisualizer/issues/1402

## Responsibilities

- Sample CPU / memory / GPU / process metrics on a periodic loop.
- Hold sensor history (ring buffers) behind a Core-owned read API.
- Publish each tick as a `MetricsSnapshot` on a single in-process broadcast bus
  that any number of subscribers (window, future tray / overlay / alerts) can
  fan out from.
- Run Core-owned persistence workers for hardware archive and storage SMART
  snapshots.
- Run a SQLite schema-version preflight check before Core applies the
  App-supplied migration set and DB-dependent workers start.
- Hold the subset of on-disk settings whose values change Core behavior.
- Expose the platform abstraction (Windows / Linux / macOS) and the
  infrastructure-level providers (sysinfo, NVAPI, WMI, procfs, …) used by it.

## Design rules

These rules are enforced by structure, not by review:

1. **No `tauri` dependency.** `core/Cargo.toml` does not list `tauri`, so any
   `use tauri::*;` under `core/src/` simply fails to compile. The dependency
   graph is the boundary.
2. **No `window.emit(...)` in Core.** Core publishes `MetricsSnapshot` to
   `EventBus`. Translating a snapshot into a Tauri event is an App-side
   concern (`src-tauri/src/adapters/window.rs`).
3. **Persistence does not share state with the collector.** Archive persistence
   subscribes to `EventBus` rather than reaching into the collector's history
   bag, so the two run as independent tasks.
4. **Settings split by consumer.** `CoreSettings` deserializes only the keys
   that affect Core (currently `hardwareArchive`). UI-only keys (`theme`,
   `language`, `lineGraph*`, `temperatureUnit`, …) stay App-side. Both crates
   read and write the same JSON file; `CoreSettings::save_to_path` merges into
   the existing object so App-owned keys survive.
5. **Presentation lives in App.** Core stores temperatures in °C; conversion
   to °F is done in the App-side adapter, not in the collector.

## Module layout

```text
core/src/
├── lib.rs
├── event_bus.rs           ← tokio broadcast<MetricsSnapshot> fan-out
├── collector/             ← sampling loop + history ring buffers
│   ├── history.rs           HistoryStore (Arc<Mutex<...>> behind a read API)
│   ├── sampling.rs          sample_system / sample_gpu cycle
│   └── system_monitor.rs    SystemMonitorController (drives the tokio task)
├── persistence/           ← Core-owned persistence workers and primitives
│   ├── archive.rs           Hardware archive writer and cleanup
│   ├── archive_data.rs      Archive row mapping helpers
│   ├── preflight.rs         DB schema-version compatibility check
│   └── storage_smart.rs     Storage SMART snapshot worker
├── settings/              ← Core-consumed settings (subset of settings.json)
│   ├── mod.rs               CoreSettings (load / save with App-key merge)
│   ├── hardware_archive.rs  HardwareArchiveSettings
│   └── storage_health.rs    Storage Health settings and identity key
├── platform/              ← Cross-platform hardware access
│   ├── traits.rs            Memory / GPU / Network / Motherboard traits
│   ├── factory.rs           PlatformFactory (compile-time OS selection)
│   ├── windows/  linux/  macos/
├── infrastructure/        ← External I/O backing the platform layer
│   ├── database/            SQLite pool + archive / SMART writers
│   └── providers/           sysinfo / NVAPI / WMI / procfs / DRM / …
├── monitoring/            ← Monitoring state types
├── models/                ← Shared data types (MetricsSnapshot, GpuMetric, …)
├── enums/                 ← Cross-cutting enums (errors, hardware, settings)
└── utils/                 ← Logger macros, formatters, IP / rounding helpers
```

Core owns persistence workers that do not need Tauri objects. The App crate
still owns Tauri-specific startup decisions: resolving the SQLite path,
supplying the ordered migration definitions, and deciding whether DB-dependent
workers can start after preflight. Core owns the pool and migration execution.

## Provider inventory

`core/src/infrastructure/providers/` contains lower-level data access helpers
used by the platform layer and Core services.

Cross-platform providers:

- `sysinfo_provider.rs` reads CPU and storage facts through `sysinfo`. The
  collector refreshes live CPU usage, memory, and process data on its periodic
  loop; the provider refreshes CPU frequency on demand when CPU hardware facts
  are requested.
- `smartctl.rs` wraps the external `smartctl` command and parses SMART data.

Windows providers:

- `windows/wmi_provider.rs` reads Windows WMI data for memory, motherboard, and
  network-related facts.
- `windows/nvapi_provider.rs` reads NVIDIA GPU data through NVAPI.
- `windows/adl_provider.rs` reads AMD GPU sensors through AMD ADL.
- `windows/pdh_provider.rs` reads GPU engine utilization through PDH.
- `windows/directx.rs` reads DXGI adapter data.
- `windows/setupdi_provider.rs` enumerates display adapters and PCI BDF
  addresses through SetupDi.
- `windows/smart.rs` reads SMART data through Windows WMI with `smartctl`
  fallback.

Linux providers:

- `linux/procfs.rs` reads procfs data such as memory facts.
- `linux/net_sys.rs` reads network interface facts from Linux system files and
  commands.
- `linux/drm_sys.rs`, `linux/hwmon.rs`, `linux/kernel.rs`, and
  `linux/lspci.rs` read GPU-related data from DRM/sysfs/debugfs/lspci sources.
- `linux/dmidecode.rs` reads DMI hardware facts through `dmidecode`.
- `linux/smart.rs` reads SMART data through `smartctl`, including Linux device
  candidate fallback.

macOS providers:

- `macos/sysctl.rs` reads low-level values through `sysctlbyname`.
- `macos/system_profiler_displays.rs` and
  `macos/system_profiler_memory.rs` parse `system_profiler` JSON output.
- `macos/gpu.rs` and `macos/gpu_info.rs` read GPU usage and adapter facts.
- `macos/net_sys.rs` reads network interface facts from macOS OS APIs.
- `macos/smart.rs` reads SMART data through `smartctl`, including diskutil
  candidate fallback.

Provider modules should stay below Core. App code should call Core APIs or App
services instead of reaching into providers directly.

## Build & test

`hardviz-core` is a workspace member. From the repository root:

```bash
# Build only the core crate
cargo build -p hardviz-core

# Run Core tests without spinning up a Tauri runtime (CI parity)
cargo nextest run -p hardviz-core --features duckdb-archive
```

CI runs the Core tests through [cargo-nextest](https://nexte.st) with the
`ci` profile from `.config/nextest.toml`. nextest runs every test in its own
process, so the DuckDB integration binaries under `core/tests/` execute in
parallel while the process-wide database path each of them initializes stays
private to the test. Install it once with `cargo install cargo-nextest --locked`.

Plain `cargo test -p hardviz-core --features duckdb-archive` still works as a
fallback. It shares one process per test binary, so it runs the integration
tests serially and takes several minutes longer.

Core is also covered by the workspace-wide `cargo tauri-fmt` /
`cargo tauri-lint` / `cargo tauri-test` aliases defined in
`.cargo/config.toml` at the repository root.

## Relationship to the App crate

```text
hardviz-core (this crate)
    │  ├─ publishes MetricsSnapshot on EventBus
    │  ├─ exposes HistoryStore read API
    │  └─ exposes SystemMonitorController
    ▼
src-tauri/ (App)
    ├─ adapters::window   ─ subscribes to EventBus, emits HardwareMonitorUpdate
    ├─ adapters::tray     ─ subscribes to EventBus, updates tray widget output
    ├─ commands::*        ─ thin delegation to Core API + Tauri input/output
    ├─ app::startup       ─ wires Core setup + DB preflight error dialog flow
    └─ workers::*         ─ owns Core controller / adapter handles for shutdown
```

## References

- Epic: [#1402 — split backend into Tauri-independent Core and thin App
  adapters][issue-1402]
- Architecture: [`docs/architecture/backend.md`](../docs/architecture/backend.md)
- App crate: [`src-tauri/README.md`](../src-tauri/README.md)
