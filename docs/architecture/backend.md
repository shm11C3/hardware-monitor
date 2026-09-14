# Backend Architecture

This document describes the current backend shape after the Core / App split.
The backend is now split across two Rust crates:

- `core/` (`hardviz-core`): Tauri-independent hardware collection, platform
  access, persistence primitives, settings consumed by Core, and shared data
  models.
- `src-tauri/` (`hardware_visualizer`): Tauri app boundary, commands, App-side
  services, event adapters, UI-only settings, lifecycle, plugins, and Tauri
  runtime wiring.

Use this document together with:

- [`core/README.md`](../../core/README.md)
- [`src-tauri/README.md`](../../src-tauri/README.md)

## High-Level Flow

The main dependency direction is:

```text
Frontend
  -> Tauri commands (`src-tauri/src/commands`)
  -> App services / Core APIs (`src-tauri/src/services`, `hardviz_core::*`)
  -> Core collector / platform / persistence (`core/src/*`)
  -> OS APIs, system providers, SQLite
```

Realtime sensor updates use a separate event flow:

```text
Core collector
  -> `hardviz_core::event_bus::EventBus`
  -> App adapters (`src-tauri/src/adapters`)
  -> Tauri events
  -> Frontend listeners
```

## Workspace Layout

```text
core/src/
├── collector/             # Sampling loop, history store, monitor controller
├── enums/                 # Core-level error, hardware, and settings enums
├── event_bus.rs           # In-process broadcast bus for MetricsSnapshot
├── infrastructure/
│   ├── database/          # Core DB initialization and access
│   └── providers/         # OS / vendor / system providers
├── models/                # Core data models, including MetricsSnapshot
├── monitoring/            # Monitoring state types
├── persistence/           # Archive, preflight, cleanup, storage health workers
├── platform/              # Platform traits, factory, and OS implementations
├── settings/              # Core-consumed settings subset
└── utils/                 # Core helpers and logging macros

src-tauri/src/
├── adapters/              # Core EventBus subscribers -> Tauri event/tray output
├── app/                   # App startup helpers
├── commands/              # Tauri command handlers and tauri-specta bindings
├── enums/                 # App-side enums exposed at the Tauri boundary
├── infrastructure/        # App-owned infrastructure, including SQL migrations
├── lifecycle.rs           # Window close / second instance / run event handling
├── models/                # App-side DTOs with specta/tauri-specta types
├── services/              # App-side business logic and Core API wrappers
├── tray/                  # Tray widget UI/runtime code
├── utils/                 # Tauri-aware helpers
└── workers/               # Handles for background controllers/adapters
```

## Crate Responsibilities

### `hardviz-core` (`core/`)

Core owns code that does not need a Tauri context:

- sampling CPU, memory, GPU, process, and related hardware metrics;
- keeping realtime history in `collector::HistoryStore`;
- publishing `MetricsSnapshot` values through `event_bus::EventBus`;
- defining platform traits and OS-specific platform implementations;
- using infrastructure providers for OS, vendor, kernel, and system data;
- running persistence workers that do not need Tauri objects;
- reading and writing settings that affect Core behavior.

Core must not depend on Tauri. This boundary is enforced by
`core/Cargo.toml`: adding `tauri` there is a design violation.

### `hardware_visualizer` (`src-tauri/`)

The App crate owns code that needs Tauri, frontend bindings, or UI-level state:

- registering Tauri commands and generating TypeScript bindings through
  `tauri-specta`;
- holding Tauri `State`, `AppHandle`, windows, plugins, and lifecycle hooks;
- converting Core events into frontend-visible Tauri events;
- applying presentation settings at the App boundary, such as temperature unit
  conversion;
- owning UI-only settings such as theme, language, graph display options,
  background image choices, tray widget settings, and close-to-tray behavior;
- defining the ordered SQL migration set supplied to Core at startup;
- holding worker/controller handles so shutdown can terminate background work.

When Close to Tray is enabled, closing the main window and exiting the process
are different lifecycle events. Code that starts, stops, flushes, or cleans up
workers must key off the process/App shutdown path when it needs finalization,
not merely the window-close path.

Elevated Startup Mode is also App-owned lifecycle behavior. It restarts the
whole Tauri process with Windows administrator privileges when enabled and the
current process is not already elevated. Core cannot be elevated independently
because it is linked into the App process; a Core-only elevation model would
require a separate helper or service process and IPC boundary.

## Layer Responsibilities

### Commands (`src-tauri/src/commands/`)

Commands are the frontend-facing IPC boundary.

Responsibilities:

- validate and normalize command inputs;
- call App services or Core APIs;
- return frontend-friendly DTOs and errors;
- participate in `tauri-specta` command collection for generated TypeScript
  bindings.

Commands should not contain sensor collection logic, OS access, long-running
worker logic, or database schema decisions.

### App Services (`src-tauri/src/services/`)

App services coordinate command-level use cases.

Responsibilities:

- wrap Tauri plugins or App-only state;
- call Core APIs where hardware data or persisted Core behavior is needed;
- keep Tauri-specific DTO conversion out of Core;
- handle UI-owned settings and presentation behavior.
- contain a mix of thin Core wrappers and App-owned services. See
  [`src-tauri/README.md`](../../src-tauri/README.md) for App crate details.

### App Adapters (`src-tauri/src/adapters/`)

Adapters subscribe to Core outputs and translate them into App outputs.

Known adapters:

- `window.rs`: receives `MetricsSnapshot` from `EventBus`, converts it into
  `HardwareMonitorUpdate`, applies the user's temperature unit, and emits the
  Tauri event.
- `tray.rs`: subscribes to the same Core event bus for tray-related output.

Core should publish data; adapters decide how that data is exposed to Tauri.

### Core Collector (`core/src/collector/`)

The collector owns realtime sampling and history.

Responsibilities:

- periodically sample system and GPU metrics;
- update `HistoryStore`;
- publish snapshots through `EventBus`;
- run under a Tokio runtime handle supplied by the App crate.

The collector does not emit Tauri events directly.

### Core Platform (`core/src/platform/`)

The platform layer abstracts OS-specific hardware access.

Responsibilities:

- define common hardware access traits in `traits.rs`;
- create the current OS implementation through `PlatformFactory`;
- keep OS-specific differences under `windows/`, `linux/`, and `macos/`;
- call infrastructure providers or OS APIs as needed.

Current platform traits include memory, GPU, network, and motherboard access.

### Core Infrastructure (`core/src/infrastructure/`)

Core infrastructure contains lower-level external access used by Core:

- `database/`: Core database initialization and DB access used by persistence;
- `providers/`: OS, vendor, and system data providers. See
  [`core/README.md`](../../core/README.md) for Core-specific provider details.

Windows sensor providers that depend on external runtime components, such as
PawnIO and its CPU-specific module blobs, are documented in
[`windows-sensor-external-components.md`](windows-sensor-external-components.md).

### Persistence (`core/src/persistence/`, `src-tauri/src/infrastructure/`)

This section describes the current row-based persistence implementation.
[ADR 0019](../adr/0019-lossless-chunked-hardware-archive.md) records the accepted
constraints for planned lossless chunked storage. Its persisted active tail,
format migration, and recurring retention maintenance are not implemented by
that decision; the startup flow and cleanup behavior below remain current.
[ADR 0021](../adr/0021-hardware-archive-migration-lifecycle.md) and the
[storage design](../development/hardware-archive-storage-design.md) record the earlier
SQLite chunk migration proposal. The current recommended direction is explained
in the [DuckDB Design Doc](../design/hardware-archive-duckdb.md), while
[#2052](https://github.com/shm11C3/HardwareVisualizer/issues/2052) tracks
investigation and delivery.

Persistence is split:

- Core owns persistence workers and DB operations that are independent of Tauri.
- App owns the ordered migration definitions and passes the resolved SQLite
  path and migration set to Core during startup.
- Core owns the database pool and executes the App-supplied migration set
  through `hardviz_core::infrastructure::database::migrate`.

Startup flow:

1. App resolves the SQLite database path.
2. App initializes Core's DB location.
3. App checks schema compatibility through Core preflight.
4. App supplies the ordered migration definitions to Core's migrator when the
   DB is compatible.
5. DB-dependent Core workers start only when startup preflight allows it.

Hardware Archive rows summarize one-minute windows of CPU, memory, GPU, and
process metrics, including available CPU and GPU temperatures and
platform-reported CPU, GPU, ANE, and package power draw. Missing temperature
and power readings remain unavailable rather than becoming zero. The
Hardware Archive Retention Period is controlled by
`hardwareArchive.retentionDays`. The `scheduledDataDeletion` flag controls
whether cleanup for records older than the Retention Period runs at startup; it
does not create a continuously scheduled deletion task. This was a deliberate
simplification from the period before close-to-tray/background execution was a
supported app behavior, when the app was not expected to stay running
continuously.

Ambient Temperature Readings are archived beside those rows in
`AMBIENT_ARCHIVE`, one row per ambient Sensor Source Label per minute, so more
than one environmental sensor can contribute to the same minute. Core owns the
`EnvironmentalSensorProvider` abstraction
(`core/src/infrastructure/providers/environmental.rs`) and knows nothing about
any vendor or transport: a provider caches whatever its transport last
delivered, and the archive tick polls that cache without blocking. The Ambient
Reading Freshness Window is five minutes
(`environmental::AMBIENT_READING_MAX_AGE_SECONDS`) - long enough that ordinary
BLE advertisement loss does not punch holes in the archive, short enough that a
sensor which stops reporting stops producing rows instead of freezing its last
value across hours. A minute with no fresh reading has no ambient row, and one
quiet sensor never suppresses another. Ambient rows carry the archive tick's
timestamp so they join the Hardware Archive row for the same minute, and they
age out on the same `hardwareArchive.retentionDays` cycle as the rows they
explain.

Which device an ambient source reads is the user's choice, never the app's.
Three SwitchBot devices in one room were observed reading between 25.2 °C and
27.3 °C - a 2 °C spread, close to half the 5 °C rise Cooling Insight reports as
a mild sustained rise - so adopting whichever advertised first picked the number every
Thermal Delta is measured against by luck, and picked differently on each
launch. The settings screen lists every device the radio is hearing with its
current reading (`get_ambient_sensor_candidates`), nothing is archived until one
is selected, and the selection is stored as that device's Bluetooth address.

That choice also settles what happens when the machine and the sensor part
company - a laptop carried to another room, or a sensor moved. The advertisement
simply stops arriving, the cached reading passes the Ambient Reading Freshness
Window, and those minutes have no ambient row and therefore no Thermal Delta.
The source is reported stale rather than substituted: a reading from a sensor in
a different room would be a confident wrong answer, and no reading is the honest
one. No signal-strength rule is involved, and none is needed.

`getAmbientArchiveSeries`
(`archive_queries::select_ambient_archive_series`) reads that archive back for
Cooling Insight's short-window timeline, bucketed on the same grid as the CPU,
power and fan series. Each bucket carries the ambient average *and* the paired
Thermal Delta, because the pairing rule below is normative and a caller handed
only the two averages could not obey it: two CTEs collapse each side to one
value per archived minute before the join, and the outer query averages the
per-minute differences. The response also names the Sensor Source Labels that
contributed to the window. There is no long-range equivalent -
`cooling_thermal_delta_daily_summary` stores each source's per-band Thermal
Delta and coverage but no ambient temperature - so the 90-day and 1-year routes
report the ambient capability as unknown rather than drawing a lane or claiming
that no sensor exists.

The provider contract is deliberately availability-based rather than
connection-based: the first concrete provider reads passive BLE advertisements
and never establishes a connection, so a link concept has no shared meaning.
A provider reports only its Sensor Source Label and its latest reading; the
registry derives Ambient Sensor Availability and the last-success timestamp for
the Cooling Insight data-state panel. Both the panel status and the rows to
write come from one evaluation per provider, so they share the same eligibility
rule: `Available` means the archive will attempt a row for that source this
minute, not that a row necessarily reached the database - the insert itself can
still fail and is logged when it does. A reading that cannot be archived at all
(no Sensor Source Label, a non-finite temperature, or a label another provider
already claimed this minute) reports as unavailable and does not advance the
last-success timestamp, so a "fresh success" the archive rejected is never
shown. The freshness window is bounded in both directions: readings stamped
more than `AMBIENT_READING_MAX_FUTURE_SKEW_SECONDS` (60 s) ahead of the tick are
refused, so a clock rewind cannot leave one reading permanently fresh.
Transport-specific causes such as an unavailable radio or a stopped scan stay
inside the concrete provider and surface only as readings that stop arriving.

The first concrete provider is the SwitchBot Meter
(`core/src/infrastructure/providers/switchbot_meter/`). It is split so only the
radio is platform-gated: `advertisement` decodes service-data bytes into a
reading, `provider` caches the newest one and answers the polling contract, and
both are portable and unit-tested from fixed byte strings on every platform.
Only `scan`, which drives a Windows BLE advertisement watcher through
`btleplug`, is `#[cfg(target_os = "windows")]`. The decode covers Meter
(device type `0x54` / `0x74`) and Meter Plus (`0x69`), whose layout and formulas
come from SwitchBot's published BLE documentation
(`OpenWonderLabs/SwitchBotAPI-BLE`, `devicetypes/meter.md`). The Outdoor Meter
is not decoded: the same document marks its layout unofficial.

The provider never connects, pairs, bonds, or writes to a meter, and there is no
SwitchBot account, cloud API, or outbound request; it reads what the device
already broadcasts. The meter publishes its service data in the scan response,
so btleplug's Windows backend runs the WinRT watcher in active scanning mode —
the radio does transmit scan requests, even though no device state is changed.
A payload whose byte 0 encryption bit is set is refused rather than decoded: its
remaining bytes are ciphertext, and ciphertext yields a perfectly plausible room
temperature whenever its low nibble lands in 0-9, which nothing downstream could
later tell from a real reading.

One Sensor Source Label has to mean one physical sensor, or every Thermal Delta
derived from it blends two rooms. The provider therefore answers for exactly one
device, and which device that is is persisted as
`environmentalSensors.switchbotMeterDevice` — the device's Bluetooth address
as twelve lowercase hex digits, the one form both the Meter path (from the
radio's peripheral id) and the Hub path (from the payload) produce. The
command that stores a choice normalizes to that form and refuses anything
else; a stored value in any other form, including the transport `Debug` string
an earlier build wrote, reads as "nothing chosen". The provider is built at
startup from the stored choice and switched in place when the user picks
another device: the cached reading is dropped with the old device so the next
archive tick cannot write it under the new label, and frames from the previous
device are refused from then on. A chosen meter that is out of range reports
unavailable rather than falling back to another one. The label carries a short
handle of the device (`SwitchBot Meter (a1b2)`), so a change of device starts
a visibly separate archive series instead of continuing an existing one.
Turning the setting off clears the choice.

The ambient registry is built once, in `setup_environmental_sensors`
(`src-tauri/src/lib.rs`), inside the `hardware_archive.enabled` branch: ambient
readings ride the archive's one-minute tick, so with the archive off there is
nowhere for a reading to go. Registration is gated on the Core-owned
`environmentalSensors.switchbotMeterEnabled` preference, which defaults to
**off** — every other source this app reads is inside the machine, and this one
turns on a radio and listens to the room. Because the registry is read-only
after startup, toggling the preference takes effect on the next launch, and the
settings screen raises the existing restart notice; choosing a device does not,
since the provider is rebound in place. While Hardware Archive is off the scan
never starts, and the settings screen says so instead of offering an empty
list. A machine with no Bluetooth
adapter, a disabled radio, or a refused scan logs once and produces no readings;
it is not surfaced as External Component Guidance, because Ambient Sensor
Availability already reports the same fact where the user is looking for it.

Process Insight data is a sampled and ranked summary derived from realtime
process observations. It is not a complete process audit log, and persistence
code should preserve that expectation unless a new feature explicitly changes
the product contract.

Storage Health Records are intentionally retained separately from the
Hardware Archive. Dashboard display can use the latest record and recent
changes, while future historical views can keep following long-term storage
health even if short-window utilization archive settings are reduced.

The cooling daily rollup (`core/src/persistence/cooling_rollup.rs`) derives one
`cooling_daily_summary` row per completed local day from the one-minute
Hardware Archive rows: CPU temperature avg/min/max per CPU-load band
(idle/low/mid/high) plus per-band sample-minute counts, the day's CPU package
power avg/min/max with its own sample-minute count, and a day coverage count.
Power is folded over the whole day rather than per band, and outside the band
gate: CPU power and CPU temperature are separate hardware capabilities, so
neither one's absence suppresses the other. It keeps its own fixed retention
window
(`cooling_rollup::COOLING_DAILY_SUMMARY_RETENTION_DAYS`, about 400 days),
independent of `hardwareArchive.retentionDays`, so Cooling Insight can show
90-day and 1-year trends without extending how long raw one-minute rows are
kept. A day with zero archived minutes has no row at all; a CPU-load band with
zero contributing minutes leaves its temperature columns absent, and a day with
no powered minute leaves its power columns absent - never zero. Its cleanup runs from the same `scheduledDataDeletion`-gated startup
site as the Hardware Archive cleanup (see ADR 0018).

The same pass folds the ambient-normalized thermal delta
(`ΔT = CPU package temperature − ambient temperature`) per CPU-load band into
`cooling_thermal_delta_daily_summary`
(`core/src/persistence/cooling_thermal_delta_rollup.rs`): one row per ambient
Sensor Source Label per completed day, keyed by `(date, source)` exactly like
the fan rollup, each carrying that source's per-band ΔT and how many archived
minutes it paired with. **The pairing rule is normative: samples are paired
first and aggregated second.** Independently aggregated CPU and ambient
summaries must never be subtracted, because the two archives do not share a
sample set - ambient readings go missing independently of hardware minutes, so
subtracting summaries built over different sample sets produces a number
corresponding to no minute that was ever observed. The rule is enforced
structurally rather than by discipline: the rollup's own read JOINs
`AMBIENT_ARCHIVE` to `DATA_ARCHIVE` on the shared one-minute timeline, one row
per `(minute, source)`, so each sample the fold sees is already one archived
minute beside one sensor's reading for that same minute. A minute with no
ambient row from a source yields no ΔT for that source, never an interpolated
one.

Row-per-source is not a convenience; it is what keeps the ΔT honest. The rollup
briefly averaged every source into one per-minute ambient value, and that
collapse is wrong for the same reason the pairing rule exists: which sensor a
ΔT was measured against *is* the measurement. Three sensors in one room were
observed about 2 K apart, close to half the 5 K rise Cooling Insight reports as
a mild sustained rise, so a per-day number that blends two placements is a ΔT no sensor
observed - and once the user switches the chosen sensor, a source-blind row
silently and irreversibly mixes the old placement and the new one, because
nothing on the row can say which minutes came from which. Keeping the source on
the row means a sensor change can never mix two placements into one baseline,
and a day with no paired minute for a source simply has no row for that source.

The two gates nest. A minute contributes to a source's ΔT band only when it
carries a classifiable CPU reading *and* that source's ambient pair, so per-band
ΔT sample minutes are always a subset of the absolute band's own. Coverage is
counted outside that nesting, the way power already is: whether the room's air
was readable is independent of whether the CPU's sensors were, so a machine
with an ambient sensor and no CPU temperature sensor still records an honest
coverage row - which is also what the backfill cursor needs, or it would re-roll
that machine's days forever.

`getCoolingBandComparison` and `getCoolingBaselineDelta` each carry an
`ambientAdjusted` variant of their result rather than a command of their own -
same question, same recent window. It carries its own lifecycle rather than a
null: a machine with no environmental sensor reports an establishing ΔT baseline
at zero qualifying days, which is honest and fabricates nothing, while
established-but-not-comparable means the reference exists and the recent window
is still too thin - or was measured against a different sensor. Both responses
also carry the ΔT baseline's own window dates, because they differ from the
absolute window the same response reports. Cooling Insight has no source picker
yet, so a window is read from whichever source covered the most of it
(`cooling_band_comparison::dominant_delta_source`), and from that source only -
never a blend.

**The ΔT baseline establishes independently of the absolute one**, over its own
window, from one ambient source, and is pinned into its own single-row
`cooling_delta_baseline` table together with that source.
This is the one place the ambient reading deviates from the absolute baseline's
design, and it is not an optimization - anchoring ΔT to the absolute baseline's
window is simply wrong. Ambient collection commonly begins *after* that window
has passed: a user adds a sensor, or the feature ships to an install with months
of history. The absolute window is then a stretch of past days with no ambient
readings, and the archive cannot grow them retroactively - so that machine would
report "not comparable" forever, no matter how much ambient data it went on to
collect. Deriving a ΔT window from days that actually carry paired minutes lets
it establish from the sensor's own first week instead.

The establishment rule runs per source, over that source's rows alone, and the
pinned baseline is only ever compared against ΔT rows of the same source. Four
qualifying days against one sensor followed by four against another establish
nothing, and a recent window from a different sensor than the baseline's is
reported but never compared, because "recent minus baseline" would then be the
difference between two placements rather than a drift in the cooling. Where
more than one source could establish, the one whose window completes first is
pinned - it is the reference that existed first.

Both baselines run the *same* establishment rule (`derive_baseline_window`,
shared so they cannot drift apart on what "established" means); they differ only
in which projection of a day they read - idle temperature versus idle ΔT - and
in the qualifying-minute bar, which for ΔT counts minutes that needed *both*
archives to produce a reading.

The two get separate tables rather than columns on one row. Pinning is
write-once (`INSERT OR IGNORE` against a `CHECK (id = 1)` row), which is exactly
what makes an established baseline undriftable; two baselines that establish at
different times cannot share one such row without the later one arriving as an
`UPDATE`, a weaker rule that must be got right rather than being impossible to
get wrong.

Each pinned window is exempt from the rollup's retention cleanup on the table it
was derived from - the absolute baseline's on `cooling_daily_summary` and
`cooling_hourly_summary`, the ΔT baseline's on
`cooling_thermal_delta_daily_summary` - and they are generally different date
ranges.

Backfill follows the lag-aware cursor precedent set by the power columns: the
catch-up claims the ΔT table is behind only when the ambient archive holds a
completed day's *pairable* reading later than any row it holds. A machine with
no ambient sensor has neither side and so never rewinds, and an ambient row
whose minute has no hardware row is excluded because re-rolling its day could
never turn it into a row.

Fan speeds are archived beside the Hardware Archive rather than inside them.
The archive worker writes one `FAN_ARCHIVE` row per fan per interval, stamped
with the write cycle's single tick instant so a fan reading and the hardware
row folded from the same snapshots cannot land in adjacent buckets. The same
rollup pass folds a completed day into one `cooling_fan_daily_summary` row per
fan (`core/src/persistence/cooling_fan_rollup.rs`). Both are row-per-fan
because how many fans a machine exposes is configuration-dependent. The
one-minute fan archive follows `hardwareArchive.retentionDays`; the daily fan
summary follows the separate cooling daily rollup retention window. The three
fan-reading meanings stay distinct end to end: an Inactive Fan Reading (0 RPM) is stored as the real
observation it is, an Invalid Fan Reading is excluded, and a missing reading
has no row - so a stopped fan is never confused with an unreadable one. There
is no hourly fan projection, because no view reads a fan axis at that
resolution.

`get_cooling_fan_trend` answers with the summarized series *and* whether the
one-minute fan archive holds any reading. An empty series alone cannot tell
"this machine has no readable fan" from "the rollup has not summarized a
completed day yet" - the state every install is in for up to a day after the
fan tables are created beside an already-full `cooling_daily_summary`, and
again on the first day of collection. The archive answers that, because it
holds a reading from the first collected minute.

The same pass also folds the day's paired co-variates
(`core/src/persistence/cooling_covariate_rollup.rs`, #2068), so Cooling
Insight can say which of package power, fan speed, ambient temperature and
load mix moved with a Thermal Delta change and which did not. Per ambient
source and CPU-load band, `cooling_covariate_daily_summary` keeps the
sufficient statistics of a least-squares fit of ΔT against CPU package power
(`n, Σx, Σy, Σxy, Σx², Σy²`) beside the day's medians of power, ΔT, ambient and
the band's share of the day; `cooling_fan_covariate_daily_summary` keeps the
same six sums for ΔT against each fan's speed, with the day's median rpm. The
sums, not the minutes, are what is stored: slope, intercept and Pearson *r* are
all functions of those six numbers, and the numbers add across days, so a
window's fit is the fit over every paired minute in it without keeping a single
minute past the Hardware Archive's own retention. Each pair inside a minute is
independent - the ΔT-power sums see only minutes carrying both readings, a
median sees every minute carrying its own - and nothing is interpolated across
minutes.

The fit is per ambient source for the reason the ΔT rollup is: the samples it
folds are the ΔT rollup's own paired read, so the ΔT it fits is the ΔT
`cooling_thermal_delta_daily_summary` holds, measured against the same sensor.
The query boundary (`cooling_covariate_comparison::load_cooling_covariate_comparison`)
reads the baseline side from the Thermal Delta Baseline's pinned window and
source, the recent side from whichever source covered most of the trailing
window, and judges nothing unless they are the same source and both windows
clear `COOLING_COVARIATE_COMPARISON_MINIMUM_PAIRED_MINUTES` - the same gate the
ambient-adjusted comparison applies, on the same windows. Each factor's recent
median is reported against the baseline window's own interquartile range of
daily medians as within range or moved; a factor never archived reports
absent, never zero. The catch-up cursor claims the co-variate tables are behind
only when the ambient archive holds a completed day's pairable reading whose
hardware minute also carries a CPU usage reading - the ΔT rule narrowed by the
one predicate this rollup's row gate adds - and the pinned ΔT baseline's window
is exempt from retention on both tables, because the comparison's baseline side
is read from exactly those rows.

## Settings Ownership

Settings are split by consumer:

- Core settings affect sampling, persistence, retention, or other Core behavior.
- App settings affect UI presentation, language/theme, graph options, tray
  behavior, window behavior, process launch behavior, and Tauri-only features.
- Both sides share a single top-level `settings.json` object. Core deserializes
  only Core-owned keys and ignores App-owned keys; App deserializes App-owned
  keys and preserves existing unknown keys when writing.
- Settings writes must merge into the existing JSON object so the writer does
  not drop keys owned by the other crate.

When adding a setting:

1. Decide whether Core or App consumes the value.
2. Persist user-facing preferences through the Rust settings service and typed
   Tauri commands.
3. Do not write user-facing application preferences directly from the frontend
   through Tauri Store.
4. Regenerate `src/rspc/bindings.ts` by running `npm run tauri:dev` when command
   types change.

## Design Rules

1. **Core has no Tauri dependency.** No `tauri` crate and no `use tauri::*`
   under `core/src/`.
2. **Commands stay thin.** Put business logic in App services or Core.
3. **Core publishes snapshots, App emits events.** `window.emit(...)` belongs in
   App adapters, not in Core.
4. **Use `PlatformFactory` for platform access.** Do not instantiate OS platform
   implementations directly from App command code.
5. **Presentation stays in App.** Core stores raw values; UI-specific conversion
   happens at the App boundary.
6. **Generated bindings are not edited by hand.** Update Rust commands and
   regenerate `src/rspc/bindings.ts`.
7. **Respect settings ownership.** Core-owned and App-owned settings may share
   the same file, but each side should preserve the other side's keys.

## Common Change Paths

### Add a New Frontend-Callable Backend Command

1. Add or update the App service in `src-tauri/src/services/`.
2. Add the command handler in `src-tauri/src/commands/`.
3. Register the command in `collect_commands![...]` in `src-tauri/src/lib.rs`.
4. Run `npm run tauri:dev` to regenerate TypeScript bindings.
5. Use the generated binding from frontend code.

### Add a New Hardware Data Source

1. Add or update the relevant Core platform trait if the capability is shared.
2. Implement OS-specific behavior under `core/src/platform/{windows,linux,macos}`.
3. Add provider code under `core/src/infrastructure/providers/` if low-level OS
   or vendor access is needed.
4. Surface the data through Core models and collector snapshots if it is
   realtime.
5. Translate Core data to App DTOs in `src-tauri/src/adapters/` or App services.

Missing or unsupported sensor data should be treated as best-effort where the
existing command/service contract allows it: prefer partial results, `None`, or
empty lists over failing an aggregate response. Check the target service before
choosing the fallback shape, because some commands still return an error for
platform initialization or required provider failures.

For vendor- or OS-dependent metrics, absence is usually a data-availability
condition rather than a fault. Preserve the distinction in new models and DTOs:
use nullable fields for unavailable per-metric values, keep source information
when it helps explain partial support, and avoid converting a partially
available device into an all-or-nothing failure unless the caller cannot
produce a useful result without that data.

External Component Guidance follows the same best-effort rule. Core may produce
structured guidance candidates when an optional runtime component such as
PawnIO or `smartctl` was actually attempted, could not be used, and fallback
collection still leaves user-visible hardware data unavailable. Those
candidates are diagnostic side data only: they must not change collection
results, fallback order, or aggregate success/failure behavior.

Core candidates should describe stable facts such as component, usage,
reason-kind, missing important signals, and optional diagnostic detail. App owns
the user-facing policy: it checks `externalComponentGuidance.acknowledgedKeys`
in `settings.json`, holds unshown candidates only in session memory, maps
guidance keys to detail URLs, returns candidates only while a relevant view can
show them, and persists acknowledgements through typed settings commands.

The initial candidate shape should stay minimal: a stable guidance key,
component, usage, reason kind, missing signal names, optional affected device
count, and optional diagnostic detail. Raw provider errors can be carried as
diagnostic detail for logs or expandable UI, but translated user-facing copy
should be driven by component, usage, and reason kind.

The initial implementation scope is limited to PawnIO for Windows CPU package
temperature and `smartctl` for Storage Health. Other vendor libraries, drivers,
or OS APIs should not be folded into this guidance path until their component,
usage, fallback behavior, and user action are explicit.
The first implementation slice should wire both initial guidance keys through
the shared candidate, settings, command, and dialog path rather than shipping
only one component first.

The minimum validation should cover both included components. PawnIO guidance
appears only when PawnIO cannot provide CPU package temperature and no ACPI CPU
temperature fallback is available. `smartctl` guidance appears only when
`smartctl` cannot be used and fallback Storage Health collection still lacks an
important health signal for at least one device. In both cases, tests should
also cover the inverse path where the optional component is unavailable but
fallback collection provides the user-visible data, so no guidance candidate is
produced.

App should aggregate unacknowledged guidance candidates by stable key. Repeated
detections of the same key replace the session-held candidate instead of
growing a queue, and already acknowledged keys are discarded before display.
If more than one unacknowledged key is relevant to the current view, the
frontend should show them one at a time.

Relevant frontend views should request pending guidance through a typed command
when they mount or become visible, rather than relying on a backend push event.
That command should filter candidates by view and return only guidance that can
be shown in the current UI context. Acknowledgement is a separate typed command
that records the guidance key in App-owned settings.

The frontend's "later" action should suppress the same guidance key only for
the current app session. It must not write an acknowledgement to settings, so a
future app session can show the guidance again if the same collection gap still
exists.

### Add or Change Persisted Data

1. Keep the App-owned ordered migration definitions in
   `src-tauri/src/infrastructure/database/`.
2. Keep Tauri-independent persistence code in `core/src/persistence/` or
   `core/src/infrastructure/database/`.
3. Update startup compatibility checks if schema compatibility changes.
4. Add tests for migration or preflight behavior where practical.

## Testing

Use the root Cargo aliases for CI parity:

```bash
cargo tauri-fmt
cargo tauri-lint
cargo tauri-test
```

For Core-only checks:

```bash
cargo build -p hardviz-core
cargo nextest run -p hardviz-core --features duckdb-archive
```

`test-core` in CI uses cargo-nextest with the `ci` profile from
`.config/nextest.toml` so the DuckDB integration binaries run in parallel.
`cargo test -p hardviz-core --features duckdb-archive` remains a slower serial
fallback when nextest is not installed. See
[`core/README.md`](../../core/README.md#build--test).

Frontend and TypeScript checks are run from the repository root:

```bash
npm run lint
npm test
```

Test placement follows the ownership boundary:

- Core pure/helper tests should live near the Core module they exercise.
- App command/service tests should live either next to the module or under
  `src-tauri/src/_tests/` when they need shared App test setup.
- Frontend tests should stay co-located under `src/**` with the component,
  hook, or feature they cover.
