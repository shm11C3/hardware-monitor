# External Components

[English](external-components.md) | [Japanese](external-components.ja.md)

This page is the source content for External Component Guidance. For now, the
app should open the GitHub-rendered version of this document directly. The
HardwareVisualizer website should later publish content derived from this page,
and the app can switch to that website URL when it exists.

Temporary in-app detail links:

- PawnIO:
  `https://github.com/shm11C3/HardwareVisualizer/blob/develop/docs/user/external-components.md#pawnio`
- smartctl:
  `https://github.com/shm11C3/HardwareVisualizer/blob/develop/docs/user/external-components.md#smartctl`

HardwareVisualizer can collect many hardware signals without optional external
components. Some deeper sensor paths depend on tools or drivers that are
installed separately. HardwareVisualizer never installs, bundles, or enables
those components automatically. On Windows, External Component Setup can
install PawnIO when you ask for it in Settings; see
[Installing PawnIO from HardwareVisualizer](#installing-pawnio-from-hardwarevisualizer).

External Component Guidance appears only after HardwareVisualizer tries to use
an optional component, cannot use it, and fallback collection still leaves
user-visible hardware data unavailable. If fallback collection obtains the
needed information, the app should not show guidance.

## What The Dialog Means

The dialog tells you:

- which optional component could not be used;
- which hardware information remains unavailable;
- what action may make that information available.

The dialog is informational. Hardware monitoring continues with the best
available fallback data.

## PawnIO

PawnIO is used on Windows for CPU package temperature and CPU package power
collection when the CPU supports the implemented PawnIO-backed path.

PawnIO may be relevant when the CPU temperature is unavailable in the dashboard.
If HardwareVisualizer can still show a CPU temperature through ACPI thermal
zones, it should not show PawnIO guidance.

### What PawnIO Enables

PawnIO can provide Windows CPU readings from CPU-specific sensor paths:

- Intel package temperature through the Intel MSR module.
- Intel CPU package power through the Intel MSR RAPL registers.
- AMD Family 17h and Family 19h package temperature through the Ryzen SMU
  module.
- AMD CPU package power through the AMDFamily17 RAPL module.

### Installing PawnIO from HardwareVisualizer

On Windows, **Settings → Advanced → Optional component setup** shows whether
the PawnIO runtime and the module files HardwareVisualizer uses are present and
offers **Install**. When you choose it, HardwareVisualizer:

1. downloads the pinned PawnIO runtime installer and the pinned PawnIO.Modules
   release from their official GitHub releases;
2. verifies each download against the size and SHA-256 digest recorded in the
   app, and discards it on a mismatch;
3. asks Windows for administrator approval, then runs the runtime installer
   unattended if the runtime is not installed yet;
4. copies only the module files that are missing into the PawnIO install
   directory. Existing files are never overwritten.

Restart HardwareVisualizer afterwards. If Windows reports that a restart is
required, restart Windows first.

Uninstalling HardwareVisualizer never removes PawnIO or its module files.
Remove PawnIO from Windows "Apps & features" if you no longer want it.

### Required User Setup

If you prefer to set up PawnIO yourself, for PawnIO-backed CPU package
temperature or power collection the machine needs:

- a working PawnIO driver/runtime installation;
- `PawnIOLib.dll`, which is provided by the PawnIO runtime installation;
- signed CPU-specific module blobs from the PawnIO.Modules release assets:
  - `IntelMSR.bin` for Intel package temperature and power;
  - `RyzenSMU.bin` for AMD package temperature;
  - `AMDFamily17.bin` for AMD package power;
- enough Windows privileges for the app process to open the PawnIO driver.

Unsigned `.amx` files are only a fallback for an unrestricted PawnIO driver in
Windows test-signing mode. They are not the normal production setup.

To set up the module blob:

1. Install the PawnIO runtime.
2. Download a release asset from
   [namazso/PawnIO.Modules](https://github.com/namazso/PawnIO.Modules/releases).
3. Extract the archive and copy the CPU module file you need into
   `C:\Program Files\PawnIO`.
   - For Intel package temperature or power, copy `IntelMSR.bin`.
   - For supported AMD package temperature, copy `RyzenSMU.bin`.
   - For AMD package power, copy `AMDFamily17.bin`.
   - If you are not sure which CPU path applies, copying all applicable signed
     module files is acceptable.
4. Restart HardwareVisualizer.

Writing to `C:\Program Files\PawnIO` usually requires administrator
permission.

If PawnIO is installed but HardwareVisualizer reports a permission issue, run
HardwareVisualizer as administrator and try again.

### Official Sources

- PawnIO runtime: <https://github.com/namazso/PawnIO>
- PawnIO modules: <https://github.com/namazso/PawnIO.Modules/releases>

More implementation detail is recorded in
[`../architecture/windows-sensor-external-components.md`](../architecture/windows-sensor-external-components.md).

### PawnIO Motherboard Sensors

PawnIO is also used on Windows for supported Super I/O motherboard temperature
and fan-speed reads. This path currently depends on the PawnIO LpcIO module and
the app process being able to open the PawnIO driver.

For PawnIO-backed motherboard sensors, the machine needs:

- a working PawnIO driver/runtime installation;
- `PawnIOLib.dll`, which is provided by the PawnIO runtime installation;
- `LpcIO.bin` or `LpcIO.amx` from the PawnIO.Modules release assets;
- enough Windows privileges for the app process to open the PawnIO driver.

If PawnIO is installed but HardwareVisualizer reports a permission issue, run
HardwareVisualizer as administrator or enable Run as administrator on startup,
then restart the app.

This guidance only covers access to the read-only motherboard sensor path. It
does not enable fan control, PWM writes, voltages, unsupported Super I/O chips,
or vendor-specific embedded-controller sensors.

## smartctl

`smartctl` is part of smartmontools. HardwareVisualizer can use it as one
Storage Health collection source when native OS paths cannot provide enough
device-health signals.

`smartctl` guidance may be relevant when Storage Health cannot collect important
health signals for at least one storage device. It should not appear if native
or fallback sources already collect the important health signals.

The `smartctl:storage-health:v1` guidance key is cross-platform. Windows,
Linux, and macOS can all surface it when HardwareVisualizer actually attempts
to use `smartctl` and still cannot collect the important Storage Health
signals through the available sources.

Live Storage Health uses the cheap native read path where available and does
not use `smartctl` on its live polling cadence.

### What smartctl Can Improve

For Storage Health, HardwareVisualizer treats these as important health signals:

- SMART overall health;
- temperature;
- NVMe percentage used;
- NVMe available spare;
- reallocated sector count;
- current pending sector count;
- offline uncorrectable sector count;
- NVMe media errors.

Other values, such as power-on hours, power cycle count, unsafe shutdown count,
or error-log entry count, may still be useful in the UI. Missing only those
values should not trigger external component guidance.

### Required User Setup

To use `smartctl`, install smartmontools for your operating system and make
sure the `smartctl` executable is available on `PATH`.

Some storage devices or operating systems may also require elevated privileges
to read SMART data. If `smartctl` is installed but HardwareVisualizer still
cannot collect Storage Health signals, try running HardwareVisualizer with the
permissions required by your operating system and device.

After installing or fixing `smartctl`, run Storage Device Refresh when
available, or restart HardwareVisualizer. Daily Storage Health collection will
also retry on its next scheduled collection.

### Official Source

- smartmontools: <https://www.smartmontools.org/>

## Guidance Reasons

The app may classify an unavailable component as:

- `missing`: the executable, DLL, or module file was not found;
- `permission`: the component exists but cannot be opened with current
  privileges;
- `misconfigured`: the runtime exists but a required driver, service, or module
  is missing;
- `failed`: the component ran but did not return usable data for the target
  device or sensor.

The website should use those classifications to show specific next steps rather
than displaying raw error strings as the main user-facing message.
