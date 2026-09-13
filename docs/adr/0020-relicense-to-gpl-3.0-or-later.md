# Relicense to GPL-3.0-or-later

Status: accepted

Tracking issue: [#2065](https://github.com/shm11C3/HardwareVisualizer/issues/2065).

This records the licensing decision agreed on 2026-09-03 and the revision from
which it applies. It is a licensing decision, not a change to product behavior.

## Context

HardwareVisualizer was published under the MIT License from its first commit.
The project now carries project-specific hardware support and analysis, such
as Cooling Insight and the clean-room native sensor implementation, and these
have become an important part of its value. The maintainer wants improvements
made to distributed derivatives of HardwareVisualizer to remain available to
the open-source community, which the MIT License does not require.

The repository state at the time of the decision:

- Every human contributor other than the maintainer contributed under the MIT
  License via the previous `CONTRIBUTING.md` clause. Those contributions are
  small (a database batching change and the Russian locale file) but remain
  part of the current code.
- No external pull requests were open, so no contribution was received under
  one license and merged under another.
- Runtime dependencies on every target platform are permissive or weak
  copyleft (MIT, Apache-2.0, BSD, ISC, Zlib, BSL-1.0, MPL-2.0, Unicode-3.0,
  CDLA-Permissive-2.0 and similar), all compatible with GPL-3.0-or-later.
  This was checked on 2026-09-03 with `cargo license` per target and the
  generated third-party notices for the npm side.
- Vendor and driver libraries (NVAPI, ADL, PawnIOLib, `smartctl`) are loaded
  from the user's system installation or run as separate processes. None of
  their code ships in the application.
- Distribution happens through GitHub Releases, the official website, and
  Winget. There is no app-store channel whose terms conflict with the GPL.

## Decision

### License and effective revision

HardwareVisualizer is licensed under the GNU General Public License v3.0 or
later (SPDX: `GPL-3.0-or-later`) from the commit that merges this ADR into
`develop`. The first release under the GPL is the next release tagged from
`develop` after that commit.

Versions and revisions published before that commit remain available under
the MIT License. This includes every release tag up to and including
`v1.10.1` and the `1.10.x` and `v1.9.x` maintenance branches. Only changes the
maintainer authored, or changes whose authors agree to MIT, may be backported
to those branches.

### Contributions

Future contributions are accepted under `GPL-3.0-or-later`; `CONTRIBUTING.md`
carries that clause.

Contributions merged before the effective revision stay licensed under the
MIT License as portions of this GPL-licensed work. The MIT License permits
that combination without further consent, but requires its notice to be kept.
The previous license text is preserved verbatim as `MIT-pre-relicense.txt` and is
bundled with the application next to `LICENSE`.

### Third-party notices

Existing third-party license notices and attribution requirements are
unchanged. Code adapted from MIT-licensed projects (the macmon-informed
IOReport reader and the Tauri documentation updater sample) keeps its file
headers and its entries in the generated `THIRD_PARTY_NOTICES.md`.

### Clean-room sensor process

The clean-room process for native CPU / Super I/O sensor work is a provenance
policy, not a consequence of the MIT License. It stays in force unchanged.
Translating or porting MPL, GPL, or LGPL monitoring implementations remains
prohibited even where those licenses are now compatible with
`GPL-3.0-or-later`; carrying ported files under file-level MPL-2.0 was
considered and rejected in #1635. `.agents/rules/clean-room-sensors.md` and
`docs/specs/sensors/README.md` state the policy in those terms.

[ADR 0023](0023-copyleft-derived-facts-for-experimental-scopes.md) later
refines one consequence of this section: because the license reason for
treating copyleft-derived register facts as non-normative no longer applies, a
specification may use such facts, tagged as lead-only, to back Experimental
scopes. The porting prohibition and the implementer's prohibited-source list
are unchanged.

### Metadata

`LICENSE` contains the verbatim GPL-3.0 text. `package.json`, `core/Cargo.toml`,
and `src-tauri/Cargo.toml` declare `GPL-3.0-or-later`. The cargo-deny license
check allows `GPL-3.0-or-later` only for the two workspace crates; the
third-party allow list is unchanged. Per-file SPDX headers are not adopted by
this decision.

## Alternatives and consequences

- Staying MIT keeps derivative improvements optional, which is the motivation
  for the change.
- AGPL-3.0 adds a network-use clause that a local desktop application does
  not need.
- MPL-2.0 or LGPL keep copyleft at file or library scope and would not cover
  the application as a whole.
- GPL-2.0-only is incompatible with the Apache-2.0 dependencies in the tree;
  GPL-3.0-or-later is not.
- Downstream projects that embedded HardwareVisualizer code under MIT can keep
  using the MIT-licensed revisions but cannot take GPL-era changes without
  accepting the GPL.
- Surfaces outside this repository that state the license must be updated
  with the first GPL release: the website, the Winget manifest `License`
  field (which `wingetcreate update` does not rewrite), the FOSSA project
  license, and a Discussions announcement.
