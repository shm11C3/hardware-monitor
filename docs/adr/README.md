# Architecture Decision Records

This directory records architectural decisions that would be hard to infer from
code alone.

ADRs are intentionally short. They explain what was decided and why; detailed
implementation guidance belongs in the architecture documents.

## Status

Every ADR carries one explicit status:

- `proposed`: direction under evaluation; do not treat it as canonical shipped
  behavior without current code/product evidence.
- `accepted`: the current decision. The agreed direction may precede its
  implementation or public release; the record must make that distinction clear.
- `superseded`: historical context replaced by a newer ADR, which the record
  must link.

ADR status describes decision maturity, not implementation or release status.

## Records

- [0001 Platform Layer for OS-specific Hardware Access](0001-platform-layer-for-os-specific-hardware-access.md)
- [0002 Core / App Split](0002-core-app-split.md)
- [0003 Storage Health Device Identity](0003-storage-health-device-identity.md)
- [0004 Separate Storage Health History](0004-separate-storage-health-history.md)
- [0005 Storage Health Naming](0005-storage-health-naming.md)
- [0006 Live Storage Health on Demand](0006-live-storage-health-on-demand.md)
- [0007 Elevated Startup Mode](0007-elevated-startup-mode.md)
- [0008 Selected Storage Device Overrides Focus Alarm](0008-selected-storage-device-overrides-focus.md)
- [0009 Generated App Hardware DTOs](0009-generated-app-hardware-dtos.md)
- [0010 Grouped Navigation with Classic Fallback](0010-grouped-navigation-with-classic-fallback.md)
- [0011 Experimental Sensor Enablement for Recognized-but-Unverified Hardware](0011-experimental-sensor-enablement.md)
- [0012 Native-first Windows Storage Health Collection](0012-native-first-windows-storage-health-collection.md)
- [0013 Centralized Live Process Polling](0013-centralized-live-process-polling.md)
- [0014 Performance Views and the Specification Sheet](0014-performance-views-and-specification-sheet.md)
- [0015 Performance and System Specifications as Sidebar Destinations](0015-performance-and-system-specifications-destinations.md)
- [0016 GPU Attribution on the Performance Screen](0016-gpu-attribution-on-the-performance-screen.md)
- [0017 Suspend Hidden Windows WebViews](0017-suspend-hidden-windows-webviews.md)
- [0018 Cooling Daily Rollup Retention](0018-cooling-daily-rollup-retention.md)
- [0019 Lossless Chunked Hardware Archive](0019-lossless-chunked-hardware-archive.md)
- [0020 Relicense to GPL-3.0-or-later](0020-relicense-to-gpl-3.0-or-later.md)
- [0021 Hardware Archive Migration Lifecycle](0021-hardware-archive-migration-lifecycle.md)
- [0022 Prefer Native DuckDB for Hardware Archives](0022-prioritize-native-duckdb-archive-qualification.md)
- [0023 External Component Setup](0023-external-component-setup.md)
