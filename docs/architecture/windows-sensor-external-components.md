# Windows Sensor External Components

This document lists the external runtime components required by Windows native
sensor collection. It is an operational checklist, not a hardware fact source.
The clean-room implementation remains derived from the pinned
`docs/specs/sensors/**` implementation-ready specs and this repository.

## CPU Package Temperature

Windows CPU package temperature collection through PawnIO requires a local
PawnIO installation. HardwareVisualizer does not bundle PawnIO. It can install
it only through External Component Setup, an explicit user action that
downloads the pinned upstream release and verifies it before running the
installer; see [ADR 0024](../adr/0024-external-component-setup.md).

Required components:

- A working PawnIO driver installation that `pawnio_open` can open.
- `PawnIOLib.dll`, loaded dynamically from the existing installation.
- One CPU-specific PawnIO module blob:
  - Intel package temperature path: signed `IntelMSR.bin`.
  - AMD Family 17h / 19h package temperature path: signed `RyzenSMU.bin`.

The CPU-specific module blob is not installed by the PawnIO runtime itself.
External Component Setup places the missing module files from the pinned
PawnIO.Modules release; users who set up PawnIO by hand download a release
asset from <https://github.com/namazso/PawnIO.Modules/releases>, extract the
module blob, and place the required file under `C:\Program Files\PawnIO`.

Production setup should use the signed `.bin` module files. An `.amx` file is
only an optional fallback for an unrestricted PawnIO driver in Windows
test-signing mode; it is not a normal end-user setup path. The module extension
does not change the CPU register decode path.

The process that opens PawnIO must have enough Windows privileges to access the
driver. On the local Phase 1 validation machine, the `PawnIO` kernel driver
service was installed and running, but a non-elevated process still failed at
`pawnio_open` with `0x80070005`. Running the same probe elevated allowed the
driver open, module load, and CPU package temperature sample to succeed. Until
HardwareVisualizer has an elevated helper or service, users who want
PawnIO-backed CPU package temperature collection across launches should enable
Elevated Startup Mode so the whole app process starts as administrator.

The current collector searches these locations:

- `InstallLocation` under
  `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\PawnIO`.
- `%ProgramFiles%\PawnIO`.
- `%ProgramW6432%\PawnIO`.
- `%ProgramFiles(x86)%\PawnIO`.

Within each candidate root, the collector searches recursively for
`PawnIOLib.dll` and the selected module file. If the DLL or module is missing,
if the DLL cannot be loaded, if the driver cannot be opened, or if the module
cannot be loaded, CPU package temperature falls back to the existing ACPI
thermal-zone path.

If both PawnIO and ACPI thermal zones are unavailable, the collector reports an
unavailable reason instead of publishing a CPU temperature. Example reasons
include:

- `PawnIOLib.dll not found`.
- `IntelMSR.bin or IntelMSR.amx not found`.
- `RyzenSMU.bin or RyzenSMU.amx not found`.
- `pawnio_open failed: ...`.
- `pawnio_load failed: ...`.

## CPU Package Power (RAPL)

Windows CPU package power uses the same local PawnIO runtime and is sampled
through the read-only RAPL MSR path. Intel temperature and power share one
`IntelMSR` executor. AMD package temperature and power use their separate
module paths.

Required module blobs are:

- Intel temperature and power: signed `IntelMSR.bin`.
- AMD package temperature: signed `RyzenSMU.bin`.
- AMD package power: signed `AMDFamily17.bin`.

The signed `.bin` files are the normal production setup. An `.amx` file may be
used only with the unrestricted PawnIO driver and Windows test-signing mode;
HardwareVisualizer does not install, bundle, or enable either form. Missing or
unusable power data remains unavailable and does not populate the derived
`package_watts` total.

## Scope Boundaries

The current implementation uses read-only CPU package temperature paths:

- Intel: `MSR_TEMPERATURE_TARGET` and `IA32_PACKAGE_THERM_STATUS` through
  `IntelMSR`.
- AMD: SMN `0x00059800` through `RyzenSMU`. Family 17h and 19h are verified;
  other families the `RyzenSMU` module recognizes (e.g. Family 1Ah / Zen 5) are
  enabled best-effort as experimental paths with the same plausibility gate.
  Successful values keep the existing Core sample, App event, and Dashboard
  presentation contracts. Verification status is maintained in the sensor
  specification; only a surfaced failure from an experimental attempt carries
  that context (see ADR 0011).

The motherboard sensor implementation also uses a read-only PawnIO LpcIO path
for the scoped Nuvoton NCT6799D Super I/O bank-4 temperature and direct RPM
registers. It requires `LpcIO.bin` or `LpcIO.amx` and the same process-level
ability to open the PawnIO driver. A non-elevated process can fail at
`pawnio_open` with `0x80070005`; in that case the Dashboard surfaces
`pawnio:motherboard-sensors:v1` guidance and offers the existing elevated
startup action.

The following are not covered by this runtime checklist:

- The External Component Setup mechanism itself (pinned artifacts, download,
  verification, installer option); it is owned by
  [`docs/design/external-component-setup.md`](../design/external-component-setup.md).
- Bundling `PawnIOLib.dll` or module blobs.
- AMD Family 1Ah / Zen 5 *verified* temperature enablement (it is enabled
  experimentally in the current implementation; verification against a
  primary source is still future work).
- AMD per-CCD temperatures or SMU PM-table metrics.
- Threadripper / EPYC multi-die-specific behavior.
- Super I/O chips outside the scoped NCT6799D read path.
- Fan control, PWM writes, voltage sensors, and embedded-controller sensors.

HardwareVisualizer downloads PawnIO components at setup time and does not
redistribute them. If a future release bundles PawnIO components instead,
update the Windows third-party notices and release packaging documentation
before shipping those artifacts.
