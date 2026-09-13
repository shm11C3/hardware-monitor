# Nuvoton NCT67xx / NCT679x Super I/O Hardware Monitor Spec

| Field | Value |
| --- | --- |
| Revision | 6 |
| Status | Implementation-ready (rev 6) |
| Scope | Phase 3 Nuvoton-family motherboard temperature and fan RPM reads for the scoped `0xD802` / `NCT6799D` Verified path and exact `0xD421` / `NCT6796D` / `NCT6796D-E` Experimental path. Revision 6 enables only read-oriented normal-HM banked access for bank 4 byte temperatures (`0x90`-`0x95`) and direct 16-bit RPM registers (`0xC0`-`0xCB`) after chip-id, LDN B, and HM-base validation, and only where S1 confirms the NCT6796D layout. It excludes exact package suffix claims such as `-R`, read-only HM access, voltage decode, count-based fan RPM decode, AUXFANIN4 / seventh fan decode, fan-control/PWM writes, threshold/limit writes, alarm clearing, GPIO, UI integration, and any Rust implementation. |
| Issue phase | Phase 3 (#1635) - Nuvoton NCT67xx / NCT679x temperature and fan RPM specification |

Revision 6 keeps this document **Implementation-ready** for two narrow
scopes. The exact `0xD802` / `NCT6799D` path remains Verified by the
independent hardware evidence described below. The official NCT6796D
datasheet identifies NCT6796D/NCT6796D-E as `0xD421` and directly pins
the normal-HM discovery, bank 4 temperature, and six direct RPM-pair
facts reused by the new Experimental path. No matching `0xD421` hardware
dump is available, so this path is not Verified and must not inherit
unconfirmed behavior from `0xD802`.

S1 does not identify `0xD802`. The `0xD802` dumps additionally expose
`0xCC`-`0xCF` as an AUXFANIN4/seventh-fan candidate; this revision does
not claim that extension for `0xD421`. The package suffix/revision for
`0xD802` remains unknown and must not be surfaced as `NCT6799D-R`.

## Sources

| ID | Source | Notes |
| --- | --- | --- |
| S1 | Nuvoton, *NCT6796D LPC/eSPI SI/O Datasheet*, version 0.6, publication release date July 6, 2017; official PDF: `https://www.nuvoton.com/resource-files/NCT6796D_Datasheet_V0_6.pdf` | Primary source for NCT6796D-only facts and family register semantics. Relevant pins: configuration protocol chapter 7, pp. 51-53; hardware-monitor LPC access chapter 8.3, pp. 54-55; temperature format chapter 8.6.3, pp. 60-62; fan speed reading chapter 8.8.1-8.8.3, pp. 66-67; fan count/RPM registers chapter 9.209-9.232, pp. 176-183; HM read-only register chapter 9.481, pp. 277-280; global chip ID CR20/CR21, p. 369; Logical Device B CR30/CR60-65, pp. 431-432. S1 does not identify `0xD802`. |
| S2 | [`superio-access.md`](superio-access.md) revision 3 | Primary clean-room source for the Phase 2 Nuvoton configuration-mode key sequence, global chip-id register reads (`0x20` / `0x21`), absent-id classification, standard Super I/O configuration port pairs, and ISA mutex policy. |
| S3 | HardwareVisualizer PR #1732 real-hardware diagnostic dump, captured 2026-06-27 on NZXT N7 B650E / AMD Ryzen 7 7800X3D / Windows 11 Pro | Independent hardware evidence for a reachable Nuvoton-class responder at slot 0 (`0x2E`/`0x2F`) with raw chip-id bytes `0xD8` / `0x02`; this is not the NCT6796D `0xD4` / `0x21` ID from S1. |
| S4 | HardwareVisualizer issue #1635 clean-room policy and [`README.md`](README.md) status-transition rules | Project policy source for keeping copyleft implementations non-normative and for allowing independently collected hardware dumps to verify facts when primary section/page references are unavailable. |
| S5 | Local standard-rights PawnIO probe on 2026-06-28 | Environment evidence only: `C:\Program Files\PawnIO` has `PawnIOLib.dll` and `LpcIO.bin`, PawnIO service is running, `pawnio_version` returned `0x00020000`, but `pawnio_open` returned `0x80070005` under medium integrity. |
| S6 | Local elevated PawnIO `LpcIO` probe on 2026-06-28, run from Administrator PowerShell on the S3-class machine | Independent hardware evidence for the pre-fix blocker: `pawnio_open`, `pawnio_load`, and `Global\Access_ISABUS.HTP.Method` mutex acquisition succeeded; slot 0 repeated `chipId=0xD802`; LDN B read `CR30=0x09`, `CR60/61=0x02/0x90` (`HM base=0x0290`), and `CR64/65=0x00/0x00` (no valid read-only HM base). `ioctl_find_bars` returned `0x80070490`, and a normal HM index-port write to `0x0295` returned `0x80070005`, so no temperature or fan RPM bytes were captured in that run. |
| S7 | [`docs/development/sensor-handoff/evidence/2026-06-28-superio-hm-dump-admin.md`](../../development/sensor-handoff/evidence/2026-06-28-superio-hm-dump-admin.md) and raw JSON captured from Administrator PowerShell on the S3-class machine | Independent local hardware evidence resolving the local HM byte-dump blocker: after slot selection, Nuvoton configuration-mode entry, LDN B selection, and `ioctl_find_bars`, bank select (`0x4E=0x04`) and all requested bank 4 reads (`0x90`-`0x95`, `0xB0`-`0xBB`, `0xC0`-`0xCF`) succeeded through normal HM ports `0x0295`/`0x0296`. |
| S8 | [`docs/development/sensor-handoff/evidence/2026-06-28-nuvoton-0xd802-source-hunt.md`](../../development/sensor-handoff/evidence/2026-06-28-nuvoton-0xd802-source-hunt.md) | Source-hunt evidence: public Nuvoton product-selection data and direct public URL probes did not expose an NCT6799D/NCT6799D-R datasheet or product page; third-party board photos and public user reports suggest NCT6799D-R, while the later independent AIDA64 dump supports `NCT6799D` for raw ID `0xD802`. |
| S9 | [`docs/development/sensor-handoff/evidence/2026-06-28-nuvoton-0xd802-aida64-dump.md`](../../development/sensor-handoff/evidence/2026-06-28-nuvoton-0xd802-aida64-dump.md), summarizing public AIDA64 text dumps attached to LibreHardwareMonitor issue #1720 | Independent dump evidence from an ASUS ROG Strix X670E-F Gaming WiFi board. The dump reports raw Super I/O device ID `D802h` and labels it `NCT6799D`; it records normal HM base `0x0290`, read-only HM base `0x0A00` on that board, bank 4 temperature bytes, and bank 4 direct RPM-style bytes. This is dump output only, not implementation source code. |
| S10 | [ADR 0011](../../adr/0011-experimental-sensor-enablement.md) | Accepted project policy source for Experimental enablement of recognized hardware when a plausible read-only path is fully defined but lacks hardware validation; it supplies no hardware-register facts. |

No LibreHardwareMonitor, OpenHardwareMonitor, Linux kernel, lm-sensors,
or decompiled monitoring-tool source is a normative source for this
revision. S9 uses public hardware dump text attached to an issue; no
AIDA64 source, binary, disassembly, or implementation structure was
consulted, and no fact below rests on LibreHardwareMonitor code.

## Validation outcome

Revision 6 passes the implementation-ready gate for the scoped
`0xD802` / `NCT6799D` Verified path and the exact `0xD421` /
`NCT6796D` / `NCT6796D-E` Experimental path:

- Exact chip identity: S9 independently maps raw `D802h` to `NCT6799D`.
  S3/S7 prove the local target board exposes the same raw chip ID. The
  package suffix/revision is not proven and remains out of scope. (S3,
  S7, S9)
- HM base and bank selection: S7 proves the local target board exposes
  LDN B normal HM base `0x0290` and can select bank 4 through normal HM
  ports after `ioctl_find_bars` succeeds. S9 corroborates normal HM base
  `0x0290` on another `D802h` / NCT6799D board. (S7, S9)
- Temperature bytes: S7 captures local bank 4 `0x90`-`0x95`; S9 captures
  the same bank/register set and a repeated temperature sampling table
  at `0490`-`0495`. (S7, S9)
- Fan/RPM bytes: S7 captures local direct RPM register bytes at
  `0xC0`-`0xCB`; S9 captures non-zero direct high/low values at the same
  six register pairs. Zero direct RPM is treated as stopped-or-unconnected,
  not as a read failure. (S7, S9)
- Experimental D421 path: S1 pins the exact chip ID and the normal-HM,
  bank 4 temperature, and six direct RPM-pair layouts used by this
  revision. No D421 hardware dump is available, so this path remains
  Experimental under ADR 0011 rather than Verified. (S1, S10)
- Remaining uncertainties are explicitly non-blocking or disabled in
  [Open questions](#open-questions) and [Scoped enablement](#scoped-enablement).

## Detection

### Chip identity and base facts

| Fact | Source |
| --- | --- |
| Use the Phase 2 Nuvoton Super I/O config-entry sequence and read global `CR20`/`CR21` as raw chip-id high/low bytes. | S2 |
| NCT6796D/NCT6796D-E's global CR20 high byte is `0xD4` and CR21 low byte is `0x21`, so its configuration-space chip ID is `0xD421`. This is the distinct Experimental scope and must not be conflated with the enabled `0xD802` scope. | S1 p. 369 |
| The observed local board in S3/S7 responded on slot 0 (`0x2E` index / `0x2F` data) with `idHigh=0xD8`, `idLow=0x02`, combined `chipId=0xD802`. | S3, S7 |
| The independent S9 AIDA64 dump reports raw device ID `D802h` and labels that responder `NCT6799D`; the same dump's per-LDN config tables show CR20/CR21 as `D8 02`. | S9 |
| The enabled runtime identity is therefore raw chip ID `0xD802` / `NCT6799D`. Do not claim package suffix `-R`, OEM variant, or package revision. | S7, S9 |
| The local S7 board reads LDN B `CR30=0x09`, normal HM base `0x0290`, and read-only HM base `0x0000`; the S9 board reads normal HM base `0x0290` and read-only HM base `0x0A00`. The enabled path uses only the normal HM base. | S7, S9 |
| On the local board, `ioctl_find_bars` failed before configuration/base discovery (`0x80070490`) but succeeded after Nuvoton configuration-mode entry and LDN B/base discovery. After that, normal HM bank select and reads through `0x0295`/`0x0296` succeeded. | S7 |

### Scoped enablement

| Scope | Status | Default enablement |
| --- | --- | --- |
| Nuvoton responder detection using Phase 2 raw chip-id bytes | Ready through `superio-access.md` rev 3; this document references the diagnostic result but does not change the Phase 2 scope. | Existing diagnostic may remain enabled. |
| NCT6796D/NCT6796D-E chip-id mapping (`0xD421`) | **Experimental**: exact identity and the compatible read-only profile are pinned by S1, but no matching hardware dump is available. No behavior is inferred from the `0xD802` identity. | Enabled as Experimental for the scoped normal-HM reads. |
| `0xD802` / `NCT6799D` chip-id mapping | **Verified** for the normal HM read scope. S7 proves the local raw ID; S9 independently maps `D802h` to `NCT6799D`. | Enabled as Verified for the scoped Phase 3 normal-HM reads. |
| Normal HM logical-device selection and base discovery | Ready for both scoped IDs where S1 defines the NCT6796D register semantics: select LDN B, read `CR30`, read `CR60/61`, validate the discovered normal HM base, then use base+`0x05`/base+`0x06`. S7/S9 additionally validate the sequence on `0xD802`. | Enabled for `0xD421` as Experimental and `0xD802` as Verified. |
| Read-only HM base (`CR64/65`) | Board-variable in evidence: local S7 is `0x0000`, S9 is `0x0A00`. The ready scope does not need it. | Disabled. |
| Bank 4 byte temperature reads `0x90`-`0x95` | S1 directly defines this NCT6796D bank/register layout; S7 and S9 independently validate the same bank/register set on `0xD802` / NCT6799D. | Enabled with generic source labels for `0xD421` as Experimental and `0xD802` as Verified. |
| Direct 16-bit fan RPM reads `0xC0`-`0xCB` | S1 directly defines these six NCT6796D direct high/low pairs; S7 and S9 independently validate the same six pairs on `0xD802` / NCT6799D. | Enabled with generic fan labels for `0xD421` as Experimental and `0xD802` as Verified. |
| Count-based fan reads `0xB0`-`0xBB` | Captured by S7/S9 but unnecessary while direct RPM registers are available. | Disabled. |
| AUXFANIN4 / seventh fan path `0xCC`-`0xCF` | Captured by S7/S9 but not required for the ready scope and not enough to assign a board-stable label. | Disabled. |

## Register map facts

### Configuration-space fields needed before hardware-monitor access

These facts are implementation-ready for the scoped `0xD421` /
NCT6796D / NCT6796D-E Experimental path and the `0xD802` / NCT6799D
Verified path unless the row explicitly says otherwise. For `0xD421`,
the S1-backed facts are a profile definition, not a hardware-validation
claim.

| Address | Name | Bits | Meaning | Units / encoding | Source |
| --- | --- | --- | --- | --- | --- |
| `0x20` | Chip ID high byte | `7:0` | `0xD8` for the `0xD802` / NCT6799D scope; `0xD4` for the exact `0xD421` / NCT6796D / NCT6796D-E Experimental scope. | Raw byte | S7, S9; S1 p. 369 |
| `0x21` | Chip ID low byte | `7:0` | `0x02` for the `0xD802` / NCT6799D scope; `0x21` for the exact `0xD421` / NCT6796D / NCT6796D-E Experimental scope. | Raw byte | S7, S9; S1 p. 369 |
| `0x07` | Logical Device Number select | `7:0` | Selects which logical device's registers are accessed at indexes `0x30` and above. | Raw logical-device number | S1 pp. 51-52, S2 |
| `0x0B` | Logical Device B / Hardware Monitor logical device | `7:0` | Selects Logical Device B, whose CR30 enables Hardware Monitor & SB-TSI and whose CR60/61 and CR64/65 define HM base addresses. | Raw logical-device number | S1 pp. 431-432; S7, S9 |
| `0x30` after LDN `0x0B` | Hardware Monitor & SB-TSI activation | bit `0` | `0` inactive, `1` active. Discovery may read this bit. This spec does not permit writing it. Higher bits are not interpreted by this revision. | Boolean active bit | S1 p. 431; S7 observed `0x09`; S9 observed `0x03` |
| `0x60` / `0x61` after LDN `0x0B` | Normal HM base address | `15:0` | Selects the normal Hardware Monitor base address. The enabled scope requires a valid base; local and independent dumps observed `0x0290`. | I/O base address | S1 p. 431; S7, S9 |
| `0x64` / `0x65` after LDN `0x0B` | Read-only HM base address | `15:0` | Optional read-only Hardware Monitor base. This revision records the value but does not use this path because evidence differs by board. | I/O base address | S1 p. 431; S7 observed `0x0000`; S9 observed `0x0A00` |

### Hardware-monitor access registers

For either scoped normal-HM path, use the base discovered from LDN B
`CR60/61` and these offsets. S1 confirms this layout for NCT6796D;
S7/S9 validate it on `0xD802`.

| Relative offset | Register | Access | Meaning | Source |
| --- | --- | --- | --- | --- |
| `+0x05` | HM index port | Write | Selects a Hardware Monitor register index. With base `0x0290`, this is port `0x0295`. | S1 pp. 54-55; S7, S9 |
| `+0x06` | HM data port | Read/write for read plumbing | Reads the selected Hardware Monitor register; also receives the bank value after selecting the bank register. With base `0x0290`, this is port `0x0296`. | S1 pp. 54-55; S7, S9 |
| HM index `0x4E` | Bank select register | Write for read plumbing | Selects the bank used by subsequent banked HM register reads. Write data value `0x04` to access bank 4. | S1 p. 176; S7, S9 |

Required writes in this table are read-transaction plumbing only. They
must not be generalized to fan-control, threshold, alarm, GPIO, or
activation writes.

### Temperature registers

Bank 4 byte temperature registers are enabled for both scoped IDs. S1
confirms this layout for the `0xD421` / NCT6796D / NCT6796D-E Experimental
profile; S7/S9 validate it for `0xD802` / NCT6799D.

| Bank | Register | Generic source label | Decode | Source |
| --- | --- | --- | --- | --- |
| `0x04` | `0x90` | SYSTIN | signed 8-bit degrees C | S1 p. 176; S7, S9 |
| `0x04` | `0x91` | CPUTIN | signed 8-bit degrees C | S1 p. 176; S7, S9 |
| `0x04` | `0x92` | AUXTIN0 | signed 8-bit degrees C | S1 p. 176; S7, S9 |
| `0x04` | `0x93` | AUXTIN1 | signed 8-bit degrees C | S1 p. 176; S7, S9 |
| `0x04` | `0x94` | AUXTIN2 | signed 8-bit degrees C | S1 p. 176; S7, S9 |
| `0x04` | `0x95` | AUXTIN3 | signed 8-bit degrees C | S1 p. 176; S7, S9 |

S7 captured these local raw bank 4 temperature bytes on the observed
NZXT N7 B650E `0xD802` board:

| Register | Raw value |
| --- | --- |
| `0x90` | `0x27` |
| `0x91` | `0x23` |
| `0x92` | `0x29` |
| `0x93` | `0x0F` |
| `0x94` | `0x13` |
| `0x95` | `0x10` |

S9 independently captured the same bank/register set on an ASUS ROG
Strix X670E-F `D802h` / NCT6799D board:

| Register | Raw value |
| --- | --- |
| `0x90` | `0x20` |
| `0x91` | `0x24` |
| `0x92` | `0x11` |
| `0x93` | `0x16` |
| `0x94` | `0x16` |
| `0x95` | `0x0E` |

### Fan tachometer / RPM registers

The rev 6 scoped paths enable direct 16-bit RPM registers only. S1
confirms the six direct pairs for the `0xD421` Experimental profile;
S7/S9 validate the same six pairs for `0xD802`. Count registers remain
disabled because direct RPM registers avoid count-to-RPM conversion
ambiguity.

| Generic fan | Bank | Direct RPM high | Direct RPM low | Decode | Source |
| --- | --- | --- | --- | --- | --- |
| Fan 1 / SYSFANIN candidate | `0x04` | `0xC0` | `0xC1` | high byte then low byte, unsigned RPM | S1 p. 66; S7, S9 |
| Fan 2 / CPUFANIN candidate | `0x04` | `0xC2` | `0xC3` | high byte then low byte, unsigned RPM | S1 p. 66; S7, S9 |
| Fan 3 / AUXFANIN0 candidate | `0x04` | `0xC4` | `0xC5` | high byte then low byte, unsigned RPM | S1 p. 66; S7, S9 |
| Fan 4 / AUXFANIN1 candidate | `0x04` | `0xC6` | `0xC7` | high byte then low byte, unsigned RPM | S1 p. 66; S7, S9 |
| Fan 5 / AUXFANIN2 candidate | `0x04` | `0xC8` | `0xC9` | high byte then low byte, unsigned RPM | S1 p. 66; S7, S9 |
| Fan 6 / AUXFANIN3 candidate | `0x04` | `0xCA` | `0xCB` | high byte then low byte, unsigned RPM | S1 p. 66; S7, S9 |

S7 local direct RPM bytes:

| Registers | Raw high/low | Direct RPM |
| --- | --- | --- |
| `0xC0`/`0xC1` | `0x00`/`0x00` | `0` |
| `0xC2`/`0xC3` | `0x0B`/`0x08` | `2824` |
| `0xC4`/`0xC5` | `0x00`/`0x00` | `0` |
| `0xC6`/`0xC7` | `0x02`/`0xE2` | `738` |
| `0xC8`/`0xC9` | `0x03`/`0x2C` | `812` |
| `0xCA`/`0xCB` | `0x00`/`0x00` | `0` |

S9 independent direct RPM bytes:

| Registers | Raw high/low | Direct RPM |
| --- | --- | --- |
| `0xC0`/`0xC1` | `0x03`/`0x59` | `857` |
| `0xC2`/`0xC3` | `0x03`/`0x2D` | `813` |
| `0xC4`/`0xC5` | `0x03`/`0x40` | `832` |
| `0xC6`/`0xC7` | `0x03`/`0x55` | `853` |
| `0xC8`/`0xC9` | `0x03`/`0x42` | `834` |
| `0xCA`/`0xCB` | `0x03`/`0x8A` | `906` |

Captured but disabled fan-related registers:

| Bank | Registers | Reason disabled |
| --- | --- | --- |
| `0x04` | `0xB0`-`0xBB` | Count-path bytes are not needed while direct RPM registers are available. |
| `0x04` | `0xCC`-`0xCF` | AUXFANIN4 / seventh fan path is outside the rev 6 ready scope. |

### D421 layout boundary

The `0xD421` profile reuses only the layout facts that S1 explicitly
defines for NCT6796D: LDN B normal-HM base discovery, normal-HM index/data
offsets, bank 4 selection, temperature registers `0x90`-`0x95`, and the
six direct RPM pairs `0xC0`-`0xCB`. S1 does not identify `0xD802`, so the
`0xD421` profile does not assume that every register observed on a
`0xD802` board has the same meaning.

In particular, S7/S9 expose `0xCC`-`0xCF` as an AUXFANIN4/seventh-fan
candidate on `0xD802`; that extra layout is not adopted for `0xD421`.
Read-only HM, count-based RPM, and any other D802-only or board-specific
extension remain disabled. No confirmed difference exists in the three
S1-backed layouts used by the Experimental profile; the difference
recorded here is the deliberate boundary against extending the D802
observation beyond those facts. (S1, S7, S9)

## Procedures

### Discovery procedure

1. Acquire `Global\Access_ISABUS.HTP.Method` with a bounded timeout
   before any Super I/O index/data or hardware-monitor I/O transaction.
   If the mutex cannot be acquired, abort the transaction rather than
   proceeding unlocked. (S2)
2. Use the Phase 2 Nuvoton configuration-mode procedure to read raw
   chip-id bytes from `0x20` / `0x21`. If the responder is absent under
   the Phase 2 absent-id rules, stop. (S2)
3. Enable the scoped normal-HM path only when the raw chip ID is
   `0xD802` or `0xD421`. Label `0xD802` as `NCT6799D` and label `0xD421`
   as `NCT6796D` / `NCT6796D-E`; do not add unproven package suffixes.
   Any other chip ID is outside this document's enabled runtime scope.
   The `0xD421` attempt is Experimental and is limited to the S1-backed
   layout boundary above. (S1, S7, S9)
4. Select LDN `0x0B`, read `CR30`, and require bit 0 to be set. Do not
   write `CR30`. (S1, S7, S9)
5. Read `CR60/61` as the normal HM base and require it to be a valid I/O
   base in the Phase 2 / PawnIO-safe range. Record `CR64/65` if useful
   for diagnostics, but do not use the read-only HM path in rev 6. (S7,
   S9)
6. Call PawnIO `ioctl_find_bars` after slot selection and LDN B/base
   discovery, before normal HM index/data I/O. On the local board this
   order changed `ioctl_find_bars` from `0x80070490` to success and
   authorized normal HM port I/O. (S7)
7. Use normal HM index port `base + 0x05` and data port `base + 0x06`.
   Select bank 4 by writing index `0x4E`, then data `0x04`. (S1, S7,
   S9)
8. Read the enabled register set: temperatures `0x90`-`0x95` and direct
   RPM pairs `0xC0`-`0xCB`. For `0xD421`, these addresses are used only
   because S1 defines them for NCT6796D. Abort the current sample on any
   failed index write or data read; do not synthesize values from partial
   reads. (S1, S7, S9)
9. Exit Nuvoton configuration mode and release the ISA mutex in all
   paths. (S2)

### Temperature decode

- Decode each enabled bank 4 temperature byte as signed 8-bit degrees C.
  Preserve the raw byte alongside the decoded value in diagnostics.
- Use generic source labels from the register table unless a later
  board-specific evidence file safely narrows physical labels.
- Do not merge motherboard/board temperature inputs with Phase 1 CPU
  package temperature sources.

### Fan RPM decode

- For enabled direct 16-bit RPM registers, combine high then low bytes
  and interpret the resulting unsigned 16-bit integer as RPM. (S1 p. 66;
  S7, S9)
- A direct RPM value of `0x0000` is a valid stopped-or-unconnected raw
  reading. Do not treat zero as a read failure and do not infer physical
  disconnection from zero alone. (S7, S9)
- Do not use count-path bytes at `0xB0`-`0xBB` or AUXFANIN4 / seventh fan
  bytes at `0xCC`-`0xCF` in the rev 6 implementation.

## Quirks

- `0xD802` is implementation-ready only as `NCT6799D` without a package
  suffix claim. Public board-review and user-report leads make
  `NCT6799D-R` plausible, but rev 6 does not prove or expose `-R`. (S8,
  S9)
- `0xD421` is implementation-ready as an Experimental
  `NCT6796D` / `NCT6796D-E` profile only for the S1-backed normal-HM,
  bank 4 temperature, and six direct RPM-pair reads. It has no matching
  hardware dump and must not be treated as Verified. (S1, S10)
- For the observed local board, `ioctl_find_bars` must run after Nuvoton
  configuration/base discovery has made the HM base visible to PawnIO
  `LpcIO`; running it before config discovery returned `0x80070490` and
  left normal HM ports unauthorized. (S7)
- Read-only HM base availability is board-variable in current evidence:
  S7 observed `0x0000`, S9 observed `0x0A00`. Rev 6 does not use the
  read-only HM path. (S7, S9)

## Safety notes

- This document is read-oriented. Required writes are limited to
  read-transaction plumbing: Nuvoton configuration-mode enter/exit,
  logical-device selection, normal HM index selection, and normal HM bank
  selection.
- Do not write activation, fan-control/PWM, Smart Fan, threshold, limit,
  alarm-clear, GPIO, or vendor/OEM registers from this spec.
- Hold the ISA mutex for the whole Super I/O / HM transaction and abort
  rather than racing another monitor.
- Require elevation/PawnIO access. If `pawnio_open` fails with
  `0x80070005`, surface an unavailable/permission diagnostic rather than
  retrying through unsafe access paths. (S5)
- Keep all disabled scopes disabled unless a later spec revision updates
  the scoped enablement table and records the necessary evidence.

## Open questions

- Non-blocking for Phase 3: A public Nuvoton NCT6799D/NCT6799D-R
  datasheet and official chip-id table remain unavailable; rev 6 is
  scoped to independently verified raw chip ID `0xD802` / `NCT6799D` and
  does not claim package suffix, OEM variant, or package revision.
- Non-blocking for Phase 3: Board-specific physical header labels remain
  unresolved; rev 6 exposes generic fan labels and preserves zero RPM as
  stopped-or-unconnected without classifying physical connection state.
- Non-blocking for Phase 3: no matching hardware dump was captured for
  `0xD421` in this validation set. The exact S1-backed read profile is
  therefore Experimental and can graduate to Verified only after a
  maintainer-accepted dump confirms the chip ID, discovered normal-HM
  base, bank 4 reads, and direct RPM pairs.
- Non-blocking for Phase 3: Read-only HM access and AUXFANIN4 / seventh
  fan decode remain disabled for both IDs; the enabled scopes use only
  normal HM bank 4 temperatures `0x90`-`0x95` and direct RPM pairs
  `0xC0`-`0xCB`.

## Implementation-ready transition checklist

Revision 6 satisfies the directory-level checklist in [`README.md`](README.md):

- no unresolved provenance marker remains in this document;
- every enabled D421 fact is pinned to S1/S2, and every enabled D802 fact
  is pinned to S1/S2 or independently verified by S7 and S9 hardware
  dumps;
- no normative fact rests solely on a copyleft implementation;
- the scoped-enablement table distinguishes the D421 Experimental path,
  the D802 Verified path, and disabled or unsupported scopes;
- every remaining open question is explicitly annotated as non-blocking
  for Phase 3; and
- the revision, status, and revision history record the ready flip.

## Provenance text for a future clean-room implementation PR

Implementation PRs should pin this ready revision and commit, for example:

```text
Implemented from docs/specs/sensors/superio-access.md revision 3 (commit a8c167b1), limited to Super I/O configuration access and chip-id diagnostic facts.
Implemented from docs/specs/sensors/superio-nuvoton-nct67xx.md revision 6 (commit <ready-commit>), limited to the `0xD421` / NCT6796D / NCT6796D-E Experimental and `0xD802` / NCT6799D Verified normal HM scopes marked enabled in that revision.
No other external sensor documentation was used.
```

## Revision history

| Revision | Date | Change |
| --- | --- | --- |
| 1 | 2026-06-28 | Initial Phase 3 Nuvoton-family draft; records Phase 2 diagnostic linkage, expected hardware-monitor spec shape, safety constraints, and unresolved provenance/dump requirements. |
| 2 | 2026-06-28 | Fetched and verified the official NCT6796D V0.6 datasheet; pinned NCT6796D configuration, HM base, temperature, and fan RPM facts; recorded that S1 maps NCT6796D to `0xD421` while the observed S3 board is `0xD802`; recorded standard-rights PawnIO dump failure (`0x80070005`). Status remains Draft. |
| 3 | 2026-06-28 | Added elevated PawnIO `LpcIO` evidence for the observed `0xD802` board: LDN B active, normal HM base `0x0290`, read-only HM base `0x0000`, and blocked HM index/data access (`ioctl_find_bars=0x80070490`, `pio_outb(0x0295)=0x80070005`). Status remains Draft. |
| 4 | 2026-06-28 | Added successful elevated normal-HM bank 4 byte-dump evidence, recorded the `ioctl_find_bars` ordering requirement, narrowed stopped/zero-RPM handling, and documented the `0xD802` source-hunt result. Exact-chip mapping remains unresolved, so status remains Draft. |
| 5 | 2026-06-28 | Added independent AIDA64 dump evidence mapping `0xD802` to `NCT6799D`, scoped support to normal HM bank 4 temperatures and direct RPM pairs, disabled unresolved suffix/read-only/AUXFANIN4 scopes, and flipped status to Implementation-ready. |
| 6 | 2026-09-13 | Added exact raw `0xD421` / NCT6796D / NCT6796D-E as an Experimental read-only scope using only the S1-confirmed normal-HM, bank 4 temperature, and six direct RPM-pair layouts; explicitly kept D802-only seventh-fan observations and other unverified extensions out of scope. Status remains Implementation-ready. |
