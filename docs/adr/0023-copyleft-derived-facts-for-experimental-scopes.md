# Copyleft-derived Facts for Experimental Sensor Scopes

Status: accepted

This record refines [ADR 0011](0011-experimental-sensor-enablement.md) and
the clean-room section of [ADR 0020](0020-relicense-to-gpl-3.0-or-later.md).
It changes how much verification weight a sensor specification may give to a
fact whose only source is a copyleft monitoring implementation. It does not
change what the clean-room implementer may read, and it does not permit
porting code.

## Context

The clean-room sensor process ([`docs/specs/sensors/README.md`](../specs/sensors/README.md))
lets the spec-author role consult MPL/GPL/LGPL monitoring implementations
(LibreHardwareMonitor, OpenHardwareMonitor, Linux hwmon, lm-sensors) only as
non-normative leads: no normative fact may rest solely on them, and a quirk
known only from such a source must stay in Open questions until independently
verified. That rule had two motivations when the repository was MIT: keeping
the resulting Rust code free of any copyleft provenance question, and keeping
register facts anchored to vendor documentation or hardware evidence.

ADR 0020 relicensed the repository to GPL-3.0-or-later and kept the clean-room
process as a provenance policy. The license motivation for treating
copyleft-derived facts as non-normative is therefore gone; the verification
motivation remains.

Meanwhile the verification rule has become the coverage bottleneck for
Cooling Insight (#1666) on self-built desktops. Motherboard fan RPM and board
temperatures are Verified for one Super I/O chip (NCT6799D, `0xD802`) and
Experimental for one more (IT8728F temperatures). Several widely used chips
have no public vendor datasheet, so the only way to reach even an Experimental
scope for them today is a user-submitted hardware dump that both identifies
the chip and labels its registers. Copyleft implementations already record
those register facts; the current rule prevents a spec from using them even
under the Experimental classification that ADR 0011 created for exactly this
"recognized but not yet verified" state.

## Decision

A **lead-only fact** is a spec fact (chip identifier, register address, bit
layout, unit, read procedure) whose only source is a copyleft monitoring
implementation read by the spec-author role and restated as a fact.

- A lead-only fact may be normative for a scope whose default enablement is
  **Experimental**. It may not back a **Verified** scope.
- Every lead-only fact is tagged: the copyleft source is listed in the
  document's Sources table with the note `lead-only (copyleft)`, the fact row
  cites that source ID, and the scoped-enablement row's Status names the
  lead-only facts it depends on together with their source IDs. A document
  whose Experimental scope rests on lead-only facts can still be
  `Implementation-ready (rev N)`.
- A lead-only scope graduates to Verified only when a primary source or a
  maintainer-accepted independent hardware dump confirms every fact the scope
  depends on; the revision history records the graduation.
- If a lead-only fact conflicts with a primary source or an accepted dump, the
  primary evidence wins and the lead-only fact moves to Open questions.
- Lead-only facts describe reads. The read-only policy, the plausibility
  gates, the mutex conventions, and the failure diagnostics of ADR 0011 apply
  unchanged; an Experimental scope built on lead-only facts is still attempted on a
  best-effort basis and still reports a failed attempt as experimental.

Everything else in the clean-room process is unchanged:

- The spec author still may not copy code excerpts, code structure, or
  implementation identifier names from any copyrighted implementation. The
  copyleft source is a source of facts, expressed in the spec author's own
  words and tables.
- The implementer still reads only `docs/specs/sensors/**` and this
  repository, with the same prohibited-source list and tool restrictions.
- Translating or porting MPL/GPL/LGPL implementation code remains prohibited,
  as ADR 0020 states.

This ADR makes no claim that reading copyleft code to learn a register fact
is license-free beyond what the existing lead policy already allowed. It
changes only the verification weight the specification may assign to such a
fact.

## Alternatives

- **Keep copyleft-derived facts non-normative.** Preserves the strongest
  verification anchor, but leaves chips without a public datasheet blocked
  until a labelled dump arrives, which is the current bottleneck.
- **Port the copyleft implementation.** Legally possible for MPL-2.0 sources
  after ADR 0020 (not for GPL-2.0-only sources such as the Linux kernel), but
  it abandons the provenance independence the project chose to keep, imports
  write paths that must then be excised, and was rejected in #1635 and again
  in ADR 0020.
- **Read sensor values at runtime from a third-party monitor** (an optional
  external component, like `smartctl`). Rejected by the maintainer: the
  product should acquire hardware readings itself.

## Consequences

- Specs gain a `lead-only (copyleft)` source tag and the status-transition
  checklist replaces "no normative fact rests solely on a copyleft source"
  with "every lead-only fact is confined to Experimental scope rows".
- Experimental coverage can widen to chips and CPU families that copyleft
  implementations describe but vendors do not document publicly. The
  user-submitted diagnostic and dump program is the graduation path.
- Code provenance is unchanged: implementations are still written from specs
  only, so the MIT maintenance branches are not affected by this decision any
  differently than by ADR 0011.
- Reviewers of a spec revision that introduces lead-only facts check the tag,
  the Experimental-only placement, and the absence of copied expression; the
  implementation PR attestations are unchanged.
