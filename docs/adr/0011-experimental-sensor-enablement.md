# Experimental Sensor Enablement for Recognized-but-Unverified Hardware

Status: accepted

We will prefer best-effort experimental enablement for hardware that the current
safe access path already recognizes, even when the register behavior has not yet
been fully verified against a primary hardware specification. The goal is to
increase the number of devices that can produce useful local readings and then
use user reports, diagnostics, and later primary-source or hardware-dump
verification to improve quality.

[ADR 0023](0023-copyleft-derived-facts-for-experimental-scopes.md) refines the
source rule for the Experimental state: a specification may rest an
Experimental scope on facts whose only source is a copyleft monitoring
implementation, tagged as lead-only, while Verified scopes still require a
primary source or an accepted hardware dump. The three-state model, the
plausibility gates, and the failure presentation below are unchanged.

The concrete trigger is issue #1824: AMD Family 1Ah / Zen 5 (Ryzen 7 9800X3D)
is recognized by the PawnIO `RyzenSMU` module, but HardwareVisualizer previously
hard-disabled it with `AMD family 0x1a is disabled by the ready spec`. Under this
policy, that family is attempted as an **experimental** CPU package temperature
source instead of being blocked only because the Family 1Ah THM facts are not yet
verified.

## Decision

For native sensor collection, the implementation distinguishes three states:

1. **Verified** — the hardware/register path is covered by the implementation-
   ready spec. The verified hardware matrix is maintained in the relevant
   sensor specification rather than exposed as routine UI state.
2. **Experimental** — the access module or OS path already recognizes the device
   or family, and HardwareVisualizer has a plausible read-only decode to attempt,
   but the exact hardware facts are not yet fully verified. The app may collect
   and display a successful reading through the existing presentation contract;
   success is not labeled or otherwise classified as experimental on screen.
3. **Unsupported** — the access module/path does not recognize the hardware, or
   attempting a read would require guessing an address, chip selection, register
   map, mutation, or behavior not already described by the current repository
   inputs. This remains disabled.

Enablement confidence is independent from `SensorAvailability`. A successfully
collected experimental value is still `Available`; `Experimental` describes the
policy for attempting the hardware/register path, not a status attached to each
successful sample.

The sensor specifications are the canonical record of which hardware is
Verified, Experimental, or Unsupported. Runtime code may mirror the minimum
classification needed to select a safe path and to describe an actual failure,
but the App event DTO and routine frontend readings do not carry verification
metadata. If collection from an Experimental path fails and an existing
diagnostic or guidance surface exposes that failure, the failure text may state
that the attempted path was experimental.

Lack of complete primary-source verification is not, by itself, a reason to
disable a reading. When the device is recognized and the existing read-only
decode can be applied without inventing hardware facts, `Experimental` is the
default. This policy applies across vendors; it is not an AMD-only exception.

For AMD CPU package temperature, this means:

- Family 17h and 19h remain **Verified** for the current `RyzenSMU` SMN
  `0x00059800` Tctl path.
- Family 1Ah / Zen 5 becomes **Experimental** when using the same read-only
  `RyzenSMU` path and existing plausibility-gated decode.
- Families not accepted by `RyzenSMU` remain **Unsupported**.

For the other current paths:

- Intel CPU package temperature has no family/model allowlist. A CPU that
  advertises the architectural DTS and package-thermal-management capability
  bits uses the existing Intel MSR path as **Verified**; the capability bits are
  existence gates, not verification allowlists.
- ACPI thermal zones are **Verified** OS/firmware API readings when they pass the
  existing plausibility filter.
- Nuvoton NCT6799D motherboard readings remain **Verified**. Other Super I/O
  chips remain **Unsupported** until an implementation-ready chip profile exists,
  because reusing a register map based only on a similar chip ID would guess
  hardware facts.
- Vendor and OS API providers that already attempt capabilities at runtime do not
  gain coverage from a family allowlist change; their existing success/failure
  contracts remain in place.

## Guardrails

This policy changes runtime enablement, not clean-room provenance:

- Do not consult prohibited monitoring implementations. Implementation still uses
  only `docs/specs/sensors/**`, this repository, and allowed platform/API docs.
- Do not add new register addresses, decode fields, chip selection logic, writes,
  fan control, limit changes, or power-state changes merely to widen coverage.
- Keep access read-only and preserve the existing mutex and optional-component
  rules.
- Keep plausibility gates. For CPU temperature, reject all-zero reads and values
  outside the accepted range rather than publishing them as experimental.
- Keep successful readings on the existing data and presentation contracts; do
  not add verification metadata, badges, or source-label suffixes to routine UI.
- When an Experimental attempt actually fails and that failure is surfaced,
  identify the attempted path as experimental in the diagnostic detail. Do not
  show an experimental label merely because collection succeeded.
- Do not show PawnIO installation/permission guidance when the only issue is
  that a source is experimental or unverified. Guidance remains for actual
  missing, permission, load, or runtime failures after an attempted source and
  all useful fallbacks are insufficient.
- Experimental support should graduate to Verified only through a later spec
  update backed by a primary source or maintainer-accepted hardware dump.

## Implementation

The implementation makes the following vertical change:

1. Core owns the internal `SensorEnablement` (`Verified`, `Experimental`,
   `Unsupported`) policy model used to select a safe path and contextualize
   failures.
2. Successful Core readings, App event DTOs, generated bindings, and frontend
   state keep their existing shapes. Verification metadata is not propagated
   with successful samples and the Dashboard receives no new label or badge.
3. AMD Family `0x1A` selects the existing AMD SMN source as `Experimental`;
   families `0x17` and `0x19` remain `Verified`, and families rejected by the
   `RyzenSMU` module remain `Unsupported`.
4. Intel remains capability-driven rather than family/model-gated.
5. A failed Experimental attempt carries that context in the existing Core
   diagnostic detail. Optional-component guidance still requires an actual
   missing, permission, load, read, or decode failure plus insufficient
   fallback data.
6. Existing plausibility checks, read-only access, mutexes, fallback behavior,
   and optional-component guidance rules remain unchanged.
7. Focused regression tests cover candidate classification, unchanged success
   labels, experimental failure wording, and guidance suppression.

The Windows-only providers still require a Windows runner or machine for final
runtime proof. Per-CCD temperatures, SMU PM-table metrics, new Zen 5 register
facts, installer bundling, and unverified Super I/O register maps remain out of
scope.
