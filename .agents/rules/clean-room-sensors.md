---
scope: "docs/specs/sensors/**,docs/development/sensor-handoff/**,core/src/infrastructure/providers/windows/pawn_io.rs,core/src/infrastructure/providers/windows/cpu_temperature.rs,core/src/infrastructure/providers/windows/cpu_temperature_decode.rs,core/src/infrastructure/providers/windows/super_io*.rs,core/src/platform/windows/motherboard.rs,core/src/platform/windows/sensors.rs,core/src/utils/super_io.rs"
---

# Clean-room rules for PawnIO sensor work (HardwareVisualizer)

These instructions enforce the clean-room (Chinese wall) process of
issue #1635 for native CPU / Super I/O sensor monitoring via PawnIO.
The canonical process description lives in
[`docs/specs/sensors/README.md`](../../docs/specs/sensors/README.md);
this file is the AI-facing enforcement summary and the committed
**prohibited-source list**.

Scope: any work on sensor specs under `docs/specs/sensors/**` and any
Rust implementation of CPU / Super I/O sensor access (MSR, SMN,
LPC/ISA port I/O, PawnIO client code).

## Roles

There are two strictly separated roles. A single session/agent must
act in exactly one role. When writing or reviewing Rust sensor code,
the **implementer** rules apply by default.

| Role | Purpose |
| --- | --- |
| Spec author ("dirty room") | Produces fact-only spec documents under `docs/specs/sensors/**` from primary sources |
| Implementer ("clean room") | Writes Rust strictly from those spec documents plus this repository |

## Prohibited sources (implementer role)

The implementer (and reviewers of implementation PRs) must NOT read,
fetch, clone, search for, quote, or otherwise consult:

- LibreHardwareMonitor / LibreHardwareMonitorLib (MPL-2.0)
- OpenHardwareMonitor (MPL-2.0)
- Linux kernel sources — in particular `drivers/hwmon/**`
  (`k10temp`, `coretemp`, `nct6775`, `it87`, …) and `arch/x86`
  MSR/SMN helpers (GPL-2.0)
- lm-sensors / `sensors-detect` (GPL-2.0 / LGPL-2.1)
- Any decompiled or disassembled monitoring tool (HWiNFO, AIDA64,
  CAM, Open Hardware Monitor forks, …)
- Forks, mirrors, vendored copies, patches, blog posts, gists, Q&A
  answers, or AI summaries that reproduce code or code structure from
  any of the above

This applies to every channel: web search, web fetch, `git clone`,
`curl`/`wget`, package contents, local checkouts, screenshots, and
content pasted into the conversation by anyone other than the
maintainer explicitly taking spec-author responsibility.

## Allowed inputs (implementer role)

- `docs/specs/sensors/**` at pinned revisions whose status is exactly
  **`Implementation-ready (rev N)`**, with no unresolved
  `TODO(provenance)` markers or blocking open questions
- This repository (code, docs, issues, PRs)
- General language/platform documentation that is not a sensor
  monitoring implementation: Rust std/crate docs, Microsoft Windows
  API documentation, `PawnIOLib.h` from an installed PawnIO release
  (upstream-published API of the driver this project calls)

If required information is missing from the specs, **stop and hand
the question to the spec-author role** (file it as an Open question /
spec revision request). Never fill spec gaps by consulting other
sensor implementations.

## Tool restrictions (implementer sessions)

- Do not use web search / web fetch tools at all.
- Do not use shell commands to fetch or clone anything from the
  prohibited-source list (no `git clone`, `curl`, `wget` of those
  projects). Dependency management (`cargo`/`npm` against their
  default registries) is allowed.
- Prefer running implementation work under the dedicated agent
  definition `.claude/agents/sensor-clean-room-implementer.md`, whose
  toolset omits web access.

## Spec-author role (summary)

Full rules: `docs/specs/sensors/README.md`. In short: vendor
datasheets / public hardware specifications / independently collected
dumps are the primary sources; MPL/GPL/LGPL implementations are leads,
and a fact resting solely on one (a **lead-only fact**, tagged
`lead-only (copyleft)`) may back an **Experimental** scope only, never
a Verified one (`docs/adr/0023-copyleft-derived-facts-for-experimental-scopes.md`);
no code excerpts, structure, or identifiers from copyrighted
implementations may enter the spec documents; every fact carries
provenance. Use `.claude/agents/sensor-spec-author.md` for this role.

## License policy

- The clean-room process is a provenance policy, not a consequence of
  the repository license. It is unchanged by the relicense to
  GPL-3.0-or-later (`docs/adr/0020-relicense-to-gpl-3.0-or-later.md`):
  all new sensor code is produced clean-room from the spec documents so
  its provenance stays independent of any third-party monitoring
  implementation.
- Translating or porting MPL/GPL/LGPL implementation code remains
  prohibited even where those licenses are now compatible with
  GPL-3.0-or-later (carrying ported files under file-level MPL-2.0 was
  considered and rejected in #1635). Restating a register fact learned
  from such an implementation in a spec, tagged lead-only and confined
  to Experimental scopes, is the spec-author allowance of ADR 0023; it
  is not porting.
- PawnIO is GPL-2.0 with an exception for independent programs
  communicating through its device IO control interface; PawnIOLib and
  the modules are LGPL-2.1-or-later. This repository calls them via
  IOCTLs and the system-installed DLL and ships none of their code, so
  its own license is unaffected. Redistributing module blobs with an
  installer requires third-party-notice compliance (see
  `docs/specs/sensors/pawnio-interface.md`).

## Hardware safety

- Sensor access is read-only. Do not write registers that alter chip
  configuration, fan control, limits, or power state.
- Honor the ISA/PCI ecosystem mutex conventions documented in
  `docs/specs/sensors/pawnio-interface.md` so concurrent monitoring tools do not
  corrupt multi-step reads.
- Fan control, PWM control, voltage control, and other hardware mutation are out
  of scope for this clean-room sensor work.

## Implementation PR requirements

No PR may be opened or reviewed as clean-room implementation work
unless all of the following hold (the "implementation gate" of
`docs/specs/sensors/README.md`):

1. Every consulted spec document is implementation-ready: it carries
   `Status: Implementation-ready (rev N)` at the pinned revision and
   has no unresolved `TODO(provenance)` markers or blocking open questions.
   The flip from draft follows the status-transition checklist in
   `docs/specs/sensors/README.md`.
2. The PR uses the clean-room PR template
   (`.github/PULL_REQUEST_TEMPLATE/clean-room-sensor-implementation.md`,
   append `?template=clean-room-sensor-implementation.md&expand=1` to the
   compare URL) and completes:
   - the spec **revision pinning** statement,
   - the implementer **provenance attestation**,
   - the reviewer **attestation** (reviewers copy the checklist into
     their approval review comment).

## Contamination handling

If a prohibited source is viewed by accident in an implementer
session (mis-click, search result, pasted content):

1. Stop implementation work in that session immediately.
2. Disclose the exposure in the PR or issue (what was seen, when).
3. The contaminated session/contributor must not write or review the
   affected implementation code; restart that work cleanly.
