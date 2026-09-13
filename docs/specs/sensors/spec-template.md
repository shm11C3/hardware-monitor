# Spec: <Domain / Chip family — short title>

<!--
Copy this file to a new lowercase kebab-case name and fill every
section. Delete the HTML comments. Keep facts and sources together.
Rules: docs/specs/sensors/README.md
-->

| Field | Value |
| --- | --- |
| Revision | 1 |
| Status | Draft — not implementation-ready |
| Scope | <what this document specifies, and what it deliberately excludes> |
| Issue phase | <phase from #1635> |

<!--
Status stays "Draft — not implementation-ready" while any
TODO(provenance) marker is unresolved. It flips to
"Implementation-ready (rev N)" only via the status-transition
checklist in README.md ("Status transition: Draft →
Implementation-ready").
-->

## Sources

<!--
Primary sources first (vendor datasheets / manuals / public hardware
specifications / independently collected hardware dumps).
MPL/GPL/LGPL implementations are leads: list them with the note
"lead-only (copyleft)". A fact whose only source is such a lead may
back an Experimental scope only (ADR 0023); Verified scopes need a
primary source or an accepted hardware dump. A lead-only fact that
conflicts with primary evidence, or that cannot be stated as a
read-only fact, belongs in Open questions. Pin page/section where
possible; otherwise add TODO(provenance).
-->

| ID | Source | Notes |
| --- | --- | --- |
| S1 | <Vendor>, *<Document title>*, document/order no. <N>, rev. <R>, §<section>/p.<page> | Primary |
| S2 | … | … |

## Detection

<!--
How an implementation decides this document applies: CPUID checks,
chip ID registers, presence probes. Each fact tagged with a source ID.
If parts of the hardware scope are unverified while the rest of the
document is ready, add a scoped-enablement table here (columns:
Scope, Status, Default enablement) following the per-family example
in cpu-amd-zen-smn.md.
-->

## Register map (facts)

<!--
Tables only. Address, name (vendor mnemonic), bit fields, units,
access width, source tag. No prose interpretation here.
-->

| Address | Name (vendor mnemonic) | Bits | Meaning | Units / encoding | Source |
| --- | --- | --- | --- | --- | --- |

## Read procedure and decode

<!--
Ordered factual steps (prose / arithmetic, no code), including required
ordering, validity checks, and the exact decode formula with units.
-->

## Quirks

<!--
Per-model deviations, errata, offsets. Each entry: factual statement +
source note backed by a primary source. A quirk known only from a
copyleft implementation must live in Open questions, not here, until
independently verified.
-->

## Safety notes

<!--
Which operations write anything (e.g. bank select), mutex requirements,
and what must never be written.
-->

## Open questions

<!--
Anything not yet verified against a primary source, with what evidence
exists so far. Implementers must treat these as unresolved.
At status-flip time every entry must be resolved or annotated as its
first line with exactly:
  Non-blocking for <phase>: <one-line justification>.
(see README.md "Status transition: Draft → Implementation-ready").
-->

## Revision history

| Revision | Date | Change |
| --- | --- | --- |
| 1 | <YYYY-MM-DD> | Initial version |
