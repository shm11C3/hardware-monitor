# Spec: Intel CPU temperature via digital thermal sensor MSRs

| Field | Value |
| --- | --- |
| Revision | 3 |
| Status | Implementation-ready (rev 3) |
| Scope | Package (and per-core) temperature on Intel x86-64 CPUs using the architectural digital thermal sensor (DTS) MSRs. Covers Nehalem-and-newer Core/Xeon/Atom parts that expose `MSR_TEMPERATURE_TARGET`. Also covers package-level live thermal status, PROCHOT#/FORCEPR# event, and power-limitation status from `IA32_PACKAGE_THERM_STATUS`; sticky/log history bits are history-only and excluded from current-state sampling. Excludes: pre-Nehalem TjMax estimation, thermal interrupt configuration, RAPL. |
| Issue phase | Phase 1 (#1635) |

## Sources

| ID | Source | Notes |
| --- | --- | --- |
| S5 | Intel SDM, **revision 092 (June 2026)**, Volume 3B, **§17.9 "Package Level Thermal Management"**, **Figure 17-33** (`IA32_PACKAGE_THERM_STATUS` layout) | Primary; package current and sticky/log status semantics |
| S6 | Intel SDM, **revision 092 (June 2026)**, Volume 4, **Table 2-2**, row `1B1H`, **p. 2-24** | Primary; register address and package-thermal-management availability |
| S1 | Intel SDM, Volume 3B, **§14.8.5.2 "Reading the Digital Sensor"** (Figure 14-31, `IA32_THERM_STATUS` layout) and **§14.9 "Package Level Thermal Management"** (Figure 14-33, `IA32_PACKAGE_THERM_STATUS` layout) | Primary; semantics and layouts |
| S2 | Intel SDM, Volume 4, **Table 2-2** (IA-32 architectural MSRs): rows `19CH` and `1B1H` (the `1B1H` row at p. Vol. 4 2-17 carries the `CPUID.06H:EAX[6]` gate and the §14.9 cross-reference); **Table 2-26**: row `1A2H` `MSR_TEMPERATURE_TARGET` | Primary; register rows |
| S3 | Intel SDM, Volume 2A, `CPUID` instruction, **leaf `06H` "Thermal and Power Management Leaf"** (p. 3-217): "Bit 00: Digital temperature sensor is supported if set" | Primary; feature detection |
| S4 | Intel, *12th Generation Intel® Core™ Processors Datasheet, Volume 1* (doc no. 655258), section "Adaptive Thermal Monitor" | Primary; 6-bit TCC Activation Offset variant (bits 29:24) on newer products |

The unchanged core DTS facts cited as S1/S2 remain pinned to the
combined-volume revision **325462-075US (June 2021)**; identifiers
can shift between revisions (e.g. §14.7.5.2 in rev -070 corresponds
to §14.8.5.2 in -075US). The `IA32_PACKAGE_THERM_STATUS` facts added
in revision 3 are pinned to Intel SDM revision **325462-092US
(June 2026)**: Volume 3B §17.9/Figure 17-33 (S5) and Volume 4
Table 2-2 p. 2-24 (S6). The cited field layouts are architectural
definitions.

## Detection

| Fact | Source |
| --- | --- |
| CPU vendor string is `GenuineIntel` (CPUID leaf 0) | S3 |
| `CPUID.06H:EAX[0] = 1` — digital thermal sensor (DTS) supported; `IA32_THERM_STATUS` readout is valid to use | S3 |
| `CPUID.06H:EAX[6] = 1` — package thermal management (PTM) supported; `IA32_PACKAGE_THERM_STATUS` exists | S3 |
| `MSR_TEMPERATURE_TARGET` (`0x1A2`) is model-specific; treat a faulted read or a zero `Temperature Target` field as "not supported" and fall back | S2 |

## Register map (facts)

All registers are read with `RDMSR` (via the PawnIO `IntelMSR` module,
see [`pawnio-interface.md`](pawnio-interface.md); all three MSRs are on
its read allow-list).

| MSR | Name | Bits | Meaning | Units / encoding | Source |
| --- | --- | --- | --- | --- | --- |
| `0x1B1` | `IA32_PACKAGE_THERM_STATUS` | 0 | Thermal Status: 1 = package temperature is at or above the package thermal threshold | flag, live/current | S5, S6 |
| `0x1B1` | `IA32_PACKAGE_THERM_STATUS` | 1 | Thermal Status Log: sticky record of Thermal Status; history-only | flag, sticky/log | S5, S6 |
| `0x1B1` | `IA32_PACKAGE_THERM_STATUS` | 2 | PROCHOT or FORCEPR# event: 1 = the package PROCHOT# or FORCEPR# condition is currently asserted | flag, live/current | S5, S6 |
| `0x1B1` | `IA32_PACKAGE_THERM_STATUS` | 3 | PROCHOT or FORCEPR# log: sticky record of the event; history-only | flag, sticky/log | S5, S6 |
| `0x1B1` | `IA32_PACKAGE_THERM_STATUS` | 10 | Power Limitation Status: 1 = package power limitation is currently active | flag, live/current | S5, S6 |
| `0x1B1` | `IA32_PACKAGE_THERM_STATUS` | 11 | Power Limitation Log: sticky record of power limitation; history-only | flag, sticky/log | S5, S6 |
| `0x19C` | `IA32_THERM_STATUS` | 22:16 | Digital Readout: temperature **below** the TCC activation temperature | °C, unsigned | S1, S2 |
| `0x19C` | `IA32_THERM_STATUS` | 30:27 | Resolution of the readout | °C | S2 |
| `0x19C` | `IA32_THERM_STATUS` | 31 | Reading Valid (1 = digital readout is valid) | flag | S2 |
| `0x1B1` | `IA32_PACKAGE_THERM_STATUS` | 22:16 | Package Digital Readout: package temperature below package TCC activation | °C, unsigned | S2 |
| `0x1A2` | `MSR_TEMPERATURE_TARGET` | 23:16 | Temperature Target: TCC activation temperature (commonly called TjMax) | °C | S2 |
| `0x1A2` | `MSR_TEMPERATURE_TARGET` | 27:24 (29:24 on newer products) | TCC Activation Offset (R/W): "a temperature offset in degrees C from the temperature target (bits 23:16); PROCHOT# will assert at the offset target temperature" | °C | S2, S4 |

Notes:

- For `IA32_PACKAGE_THERM_STATUS`, bits 0, 2, and 10 are live/current
  status fields. Bits 1, 3, and 11 are their sticky/log history fields;
  they are history-only and are excluded from current-state sampling.
  No current-state signal may be inferred from those log bits, and this
  specification defines no write-to-clear operation. (S5)

- `IA32_THERM_STATUS` is a **per-core** register; the readout reflects
  the core the `RDMSR` executes on. (S1)
- `IA32_PACKAGE_THERM_STATUS` is **package-scope**: any logical CPU of
  the package reads the same value. It has **no** Reading Valid bit.
  (S2)
- The `TCC Activation Offset` field is 4 bits (27:24) in the SDM
  rev -075US model tables (S2); newer product datasheets document a
  6-bit field at 29:24 (S4). The decode in this document never
  consumes the field, so the width difference does not affect Phase 1.

## Read procedure and decode

1. Confirm detection facts above.
2. Read `MSR_TEMPERATURE_TARGET` (`0x1A2`) once per session and
   extract the Temperature Target (`t_target`) from bits 23:16. If
   the read faults or `t_target` is 0, report "unsupported" (caller
   falls back to ACPI zones per #1633).
3. Package temperature (preferred for Phase 1): read
   `IA32_PACKAGE_THERM_STATUS` (`0x1B1`). Decoding rules (hardware
   semantics):
   - Extract the Package Digital Readout from bits 22:16.
   - Package temperature = `t_target` − readout, in °C.
   - From the same 64-bit read, decode the live package status fields:
     - Bit 0 is Thermal Status: set means the package temperature is at
       or above the package thermal threshold.
     - Bit 2 is the PROCHOT or FORCEPR# event: set means the package
       PROCHOT# or FORCEPR# condition is currently asserted.
     - Bit 10 is Power Limitation Status: set means package power
       limitation is currently active.
   - Bits 1, 3, and 11 are sticky/log history fields. Do not use them as
     current-state signals, and do not write the MSR to clear them. (S5, S6)
4. Per-core temperature (optional, later): read `IA32_THERM_STATUS`
   (`0x19C`) on the target core; use the value only when bit 31
   (Reading Valid) is 1; decode with the same subtraction.
5. Plausibility gate before publishing (defensive, this project's own
   policy, not an SDM fact): accept only `0 < temp ≤ t_target`
   (inclusive — a readout of 0 decodes to `temp == t_target` and
   passes the gate) and `t_target` within 50–120 °C; otherwise drop
   the sample.

## Quirks

- Pre-Nehalem parts (e.g. Core 2) do not document a usable
  `MSR_TEMPERATURE_TARGET`; tools of that era guessed TjMax. Such
  parts are **out of scope**: the `t_target == 0` / fault rule above
  excludes them. (S2)
- The digital readout saturates: values at or above TCC activation
  read as 0 °C below target. A readout of 0 therefore decodes to
  `temp == t_target` ("at TjMax"); the plausibility gate accepts it
  and implementations publish it as-is (with the Reading Valid bit
  checked where the register defines one). Sustained zero readouts on
  an otherwise idle system indicate a stuck or failed sensor read and
  are worth surfacing in logs, but this spec does not require dropping
  them. (S1)
- TCC Activation Offset does not enter the temperature calculation:
  S2 defines the offset as "a temperature offset in degrees C **from
  the temperature target (bits 23:16)**; PROCHOT# will assert at the
  offset target temperature" — i.e. bits 23:16 stay the fixed
  reference and a programmed offset moves the throttle-assertion
  point below it. The decode therefore always uses `t_target` =
  bits 23:16. (S1, S2; empirical confirmation on offset-programmed
  machines tracked under Open questions)
- Multi-package systems: `0x1B1` must be read once per package, with
  the reading thread affinity-pinned to a core of each package. Phase
  1 targets single-package consumer machines (package 0). PawnIO
  executes `RDMSR` on the calling thread's current processor, so the
  client's thread affinity selects the package — see
  [`pawnio-interface.md`](pawnio-interface.md). (S2)

## Safety notes

- Read-only: `RDMSR` of the three listed MSRs only. No `WRMSR` in any
  phase. The PawnIO `IntelMSR` module's write surface is not used.
- MSR reads have no bus-level side effects requiring the ISA or PCI
  mutex conventions.
- Sticky/log bits 1, 3, and 11 are history-only; do not write them to
  clear history or treat them as current-state samples. (S5)

## Open questions

- Non-blocking for Phase 1: the decode uses `t_target` = bits 23:16
  per the resolved Quirks entry; empirical behavior on machines with
  a nonzero programmed TCC Activation Offset is additionally
  confirmed via Phase 2 dumps.
- Non-blocking for Phase 1: only the package-scope readout (`0x1B1`)
  is used, and it is architecturally identical on hybrid (P/E-core)
  parts; per-core readings are out of scope. Verify per-core-type
  behavior before any later per-core phase.

## Revision history

| Revision | Date | Change |
| --- | --- | --- |
| 1 | 2026-06-10 | Initial version |
| 2 | 2026-06-11 | Provenance pinned to SDM 325462-075US (June 2021): §14.8.5.2/§14.9, Table 2-2 rows 19CH/1B1H, Table 2-26 row 1A2H, Vol 2A leaf 06H. TCC-offset readout-reference question resolved (offset moves PROCHOT only); RDMSR execution-context question resolved via PawnIO facts. Remaining open questions annotated non-blocking. Status → Implementation-ready. |
| 3 | 2026-09-13 | Issue #2114 verified revision: added Intel SDM 325462-092US (June 2026) provenance for `IA32_PACKAGE_THERM_STATUS` via §17.9/Figure 17-33 and Volume 4 Table 2-2 p. 2-24; pinned live/current bits 0, 2, and 10 and sticky/log bits 1, 3, and 11. Live bits are safe for current-state sampling; sticky/log bits are excluded from implementation. |
