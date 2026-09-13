# Sensor Hardware Specifications (Clean-Room)

This directory is the specification library for native CPU and Super I/O
sensor monitoring via [PawnIO](https://github.com/namazso/PawnIO)
(issue #1635). Every document here is a **fact-only hardware/interface
specification** produced by the "spec author" role of the clean-room
process described below.

These documents are the **only** external technical input the
implementation role is allowed to use. Keeping them factual, sourced,
and free of third-party code is what keeps the provenance of the
resulting Rust code independent of third-party monitoring
implementations. That requirement is a project policy, not a
consequence of the repository license, and it is unchanged by the
relicense to GPL-3.0-or-later
([ADR 0020](../../adr/0020-relicense-to-gpl-3.0-or-later.md)).

## Clean-room process (two roles)

| Role | May read | Must not do |
| --- | --- | --- |
| Spec author ("dirty room") | Vendor datasheets and manuals (primary); public hardware specifications; independently collected hardware dumps; MPL/GPL/LGPL implementations as leads, whose facts may back **Experimental** scopes only when tagged lead-only ([ADR 0023](../../adr/0023-copyleft-derived-facts-for-experimental-scopes.md)) | Copy code excerpts, code structure, or implementation identifier names into spec documents |
| Implementer ("clean room") | `docs/specs/sensors/**` and this repository only | Read LibreHardwareMonitor / OpenHardwareMonitor / Linux kernel / lm-sensors sources, or any decompiled monitoring tool |

Names that are part of a public API contract (for example PawnIO module
function names such as `ioctl_read_msr`) are interface facts required
for interoperability, not implementation identifiers; they may appear
in spec documents.

## Hard rules for documents in this directory

- State **facts**, with a source note (provenance) for each fact or
  fact group: document title, document/order number, and section or
  page where known. Use `TODO(provenance)` when a page-level citation
  still needs to be pinned.
- No code excerpts, no code structure, and no identifier names taken
  from copyrighted implementations.
- MPL/GPL/LGPL implementations are leads. A fact whose only source is
  such an implementation is a **lead-only fact**: it may be normative
  only for a scope whose default enablement is **Experimental**, and it
  may never back a **Verified** scope
  ([ADR 0023](../../adr/0023-copyleft-derived-facts-for-experimental-scopes.md)).
  List the copyleft source in the Sources table with the note
  `lead-only (copyleft)`, cite its ID on each lead-only fact row, and
  name the lead-only facts in the scoped-enablement row that depends
  on them. Verified scopes still require vendor documentation, public
  hardware specifications, or maintainer-accepted independent hardware
  dumps.
- A lead-only fact that conflicts with a primary source or a
  maintainer-accepted hardware dump moves to **Open questions**; the
  primary evidence wins.
  A quirk known only from a copyleft implementation that cannot be
  expressed as a read-only fact for an Experimental scope stays in
  Open questions until independently verified.
- Anything uncertain goes in the document's **Open questions** section,
  not in the fact tables.
- Read-only orientation: documents describe register *reads*. Writes
  are documented only where a read transaction requires them (for
  example configuration-mode entry keys or bank selection), and must be
  marked as such.

## Document conventions

- One document per access domain or chip family, lowercase kebab-case
  filenames (see [`docs/documentation-guide.md`](../../documentation-guide.md)).
- Start from [`spec-template.md`](spec-template.md).
- Each document carries a **revision number** and a revision history
  table. Any change to facts increments the revision.
- Implementation PRs must pin the spec they were built from in the PR
  body, e.g.:

  ```text
  Implemented from docs/specs/sensors/cpu-amd-zen-smn.md revision 1
  (commit <sha>). No other external sensor documentation was used.
  ```

  This is the audit trail demonstrating clean-room provenance.

## Document status and implementation gate

Documents start as **draft specifications** and become valid
clean-room inputs only through the status transition below (with the
Phase 0 guardrails merged, as listed above). Per-document status is
tracked in each document's header and in the Current documents table.

- A document containing `TODO(provenance)` markers must carry
  `Status: Draft — not implementation-ready` and must not be used as
  the sole clean-room input for implementation until all markers are
  resolved and primary-source section/page references are pinned (or
  the facts are otherwise independently verified, e.g. against
  hardware dumps).
- No implementation PR may be opened or reviewed as clean-room work
  until all of the following Phase 0 guardrails exist:
  - `.agents/rules/` contains the prohibited-source list —
    satisfied by
    [`clean-room-sensors.md`](../../../.agents/rules/clean-room-sensors.md)
  - `CLAUDE.md` references the clean-room implementer restrictions —
    satisfied by the instruction-file entry in
    [`CLAUDE.md`](../../../CLAUDE.md)
  - the PR template requires spec revision pinning, a provenance
    attestation, and the reviewer attestation below — satisfied by
    [`clean-room-sensor-implementation.md`](../../../.github/PULL_REQUEST_TEMPLATE/clean-room-sensor-implementation.md)
  - dedicated role agents with tool restrictions exist — satisfied by
    [`.claude/agents/sensor-spec-author.md`](../../../.claude/agents/sensor-spec-author.md) and
    [`.claude/agents/sensor-clean-room-implementer.md`](../../../.claude/agents/sensor-clean-room-implementer.md) (the
    implementer agent has no web tools)

  The artifacts above satisfy the gate's existence requirements;
  per-document readiness (`TODO(provenance)` resolution) still gates
  each individual spec.
- Reviewer contamination policy: reviewers can also breach the
  clean-room boundary. Implementation PR reviews must include this
  attestation:

  ```markdown
  - [ ] I reviewed this implementation only against
        `docs/specs/sensors/**`, this repository, and the pinned spec
        revision.
  - [ ] I did not consult LibreHardwareMonitor, OpenHardwareMonitor,
        Linux kernel, lm-sensors, or decompiled monitoring tools while
        reviewing this implementation.
  ```

  Reviewers copy this checklist, with both boxes checked, into their
  approval review comment. The implementation PR template carries the
  checklist as a reminder of this requirement.

## Status transition: Draft → Implementation-ready

A document becomes a valid clean-room input only through this
transition. The flip is proposed by the spec-author role and approved
by a maintainer; the sign-off is the maintainer's approval of the PR
that performs the flip.

Checklist for the flipping PR (all items required):

- [ ] Every `TODO(provenance)` marker is resolved: each affected fact
      is pinned to a primary-source section/page, or independently
      verified (e.g. against a hardware dump referenced by the
      document).
- [ ] Every entry under **Open questions** is either resolved (moved
      into the fact tables with provenance) or explicitly annotated
      in place, as the first line of the entry, using exactly this
      form:

      ```text
      Non-blocking for <phase>: <one-line justification>.
      ```

      Example: `Non-blocking for Phase 1: package readout does not
      depend on this; only per-core readings would.` The phase name
      and justification stay in the Open questions section.
- [ ] Every fact that rests solely on a copyleft source is tagged
      lead-only (Sources note `lead-only (copyleft)`, source ID on the
      fact row) and is depended on only by scoped-enablement rows whose
      default enablement is Experimental; no Verified scope depends on
      one (re-check the notes column of the Sources table).
- [ ] Scoped-enablement tables are consistent with the verification
      state of each row. A *scoped-enablement table* is the pattern
      for documents that are ready overall while specific hardware
      scopes are not: a table in the **Detection** section with
      columns `Scope` (e.g. CPU family or chip model), `Status` (what
      verified the row, with source tag), and `Default enablement`
      (enabled, or disabled until a named verification happens). The
      per-family table in [`cpu-amd-zen-smn.md`](cpu-amd-zen-smn.md)
      is the reference example.
- [ ] The revision number is bumped, the revision history records the
      transition, and the Status field is set to
      **`Implementation-ready (rev N)`** — this is the canonical
      ready value that implementer and reviewer attestations check
      for.

Implementers and reviewers verify readiness by checking the Status
field at the pinned revision; any remaining `TODO(provenance)` marker
or unresolved blocking open question invalidates the flip.

## Current documents

| Document | Covers | Issue phase | Status |
| --- | --- | --- | --- |
| [`pawnio-interface.md`](pawnio-interface.md) | PawnIO driver/library API, module IOCTL contracts (`IntelMSR`, `RyzenSMU`, `AMDFamily17`, `LpcIO`), mutex conventions, blob distribution (signed `.bin`), elevation requirement, licensing facts | Phase 1 | Implementation-ready (rev 6) |
| [`cpu-intel-dts-msr.md`](cpu-intel-dts-msr.md) | Intel digital thermal sensor via MSRs (package/core temperature) | Phase 1 | Implementation-ready (rev 2) |
| [`cpu-amd-zen-smn.md`](cpu-amd-zen-smn.md) | AMD Zen Tctl/Tdie via SMN thermal controller | Phase 1 | Implementation-ready (rev 4) |
| [`cpu-intel-rapl-msr.md`](cpu-intel-rapl-msr.md) | Intel CPU package power via RAPL energy MSRs (`0x606`/`0x611`), unit decode, 32-bit wraparound and wrap-safe gap handling, Silvermont-unit exclusion | Phase 5 | Implementation-ready (rev 3) |
| [`cpu-amd-zen-rapl-msr.md`](cpu-amd-zen-rapl-msr.md) | AMD Zen (17h/19h/1Ah) CPU package power via RAPL energy MSRs (`0xC0010299`/`0xC001029B`), per-model counter widths, width-agnostic modular decode with wrap-safe gap handling, 1Ah 44h domain-semantics open question | Phase 5 | Implementation-ready (rev 3) |
| [`superio-access.md`](superio-access.md) | Phase 2 raw Super I/O chip-id diagnostic: config port pairs, Nuvoton/ITE enter/exit, chip-id registers, absent-id classification, ISA mutex | Phase 2 | Implementation-ready (rev 3) |
| [`superio-nuvoton-nct67xx.md`](superio-nuvoton-nct67xx.md) | Nuvoton NCT67xx/NCT679x hardware-monitor map for motherboard temperatures and fan RPM. Rev 5 is implementation-ready for scoped `0xD802` / `NCT6799D` normal HM bank 4 byte temperatures (`0x90`-`0x95`) and direct RPM pairs (`0xC0`-`0xCB`), validated by local elevated PawnIO dump plus an independent AIDA64 dump. NCT6796D, read-only HM, count-based RPM, AUXFANIN4/seventh fan, voltages, and PWM remain disabled or out of scope. | Phase 3 | Implementation-ready (rev 5) |
| [`superio-ite-it86xx-it87xx.md`](superio-ite-it86xx-it87xx.md) | ITE Environment Controller discovery and read-only motherboard temperatures. Rev 2 is implementation-ready only for exact raw chip ID `0x8728` / IT8728F/EX generic `TMPIN1`-`TMPIN3`, enabled as Experimental pending a user-submitted hardware dump. It requires an explicit post-exit EC index/data-port authorization read before caching the PawnIO path. FAN1-5, voltages, controls, physical labels, and every other IT86xx/IT87xx ID remain disabled or unsupported. | Phase 4 | Implementation-ready (rev 2) |

The Nuvoton Phase 3 document is implementation-ready only for the scoped
`0xD802` / `NCT6799D` normal HM read path listed above. The ITE Phase 4
document is implementation-ready only for the exact `0x8728` Experimental
temperature path; it does not make the broader IT86xx/IT87xx family or any
ITE fan path ready. The current `superio-access.md` readiness remains
intentionally limited to the Phase 2 raw chip-id diagnostic scope.

## Safety policy (applies to all documents and implementations)

- Read-only register access. No writes that alter chip configuration,
  fan control, limits, or power state in any phase of #1635.
- Honor the ecosystem mutex conventions
  (`Global\Access_ISABUS.HTP.Method`, `Global\Access_PCI`) so that
  concurrent monitors (HWiNFO, LibreHardwareMonitor, FanControl) do not
  corrupt each other's multi-step read transactions. Details in
  [`pawnio-interface.md`](pawnio-interface.md).
- When PawnIO is not installed, the application degrades gracefully to
  the ACPI thermal-zone path introduced by PR #1633.
