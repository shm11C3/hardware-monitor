# Project Documentation

This directory contains project documentation that is not part of the primary
user-facing root README.

Start here when looking for development, architecture, release, or maintenance
documentation.

## Main Entry Points

- [Product and design principles](design-principles.md)
- [Backend architecture](architecture/backend.md)
- [External components](user/external-components.md) /
  [Japanese](user/external-components.ja.md)
- [Windows sensor external components](architecture/windows-sensor-external-components.md)
- [Architecture decision records](adr/)
- [Lossless chunked Hardware Archive decision](adr/0019-lossless-chunked-hardware-archive.md)
- [Hardware Archive migration lifecycle proposal](adr/0021-hardware-archive-migration-lifecycle.md)
- [Native DuckDB direction decision](adr/0022-prioritize-native-duckdb-archive-qualification.md)
- [Native DuckDB Hardware Archive Design Doc](design/hardware-archive-duckdb.md)
- [Native DuckDB distribution and durability evidence](development/hardware-archive-duckdb-distribution-evidence.md)
- [Earlier SQLite chunk candidate](development/hardware-archive-storage-design.md)
- [Hardware Archive investigation and work tracking](https://github.com/shm11C3/HardwareVisualizer/issues/2052)
- [Relicense to GPL-3.0-or-later decision](adr/0020-relicense-to-gpl-3.0-or-later.md)
- [Sensor hardware specs (clean-room)](specs/sensors/)
- [Frontend architecture](../src/README.md)
- [Core crate guide](../core/README.md)
- [Tauri app crate guide](../src-tauri/README.md)
- [Add a new language](development/add-language.md)
- [E2E capture harness](development/e2e-captures.md)
- [Super I/O sensor work handoff](development/sensor-handoff/)
- [GitHub label guide](development/labels.md)
- [Download verification](download-verification.md)
- [Release vulnerability response](security/release-vulnerability-response.md)
- [Documentation guide](documentation-guide.md)
- [Shared agent rules](../.agents/rules/README.md)
- [AI learning records](agents/lessons/)

## Documentation Map

```text
docs/
├── README.md                       # Documentation index
├── design-principles.md            # Product and engineering decision lens
├── documentation-guide.md          # Documentation placement and naming rules
├── agents/                         # AI learning provenance and promotion records
├── adr/                            # Architecture decision records
├── architecture/                   # Architecture documents
├── development/                    # Developer task guides
├── specs/                          # Clean-room hardware specification documents
│   └── sensors/                    # Sensor specs for PawnIO-based monitoring
├── security/                       # Release security response and evidence policy
├── licenses/                       # License info, generated + manual third-party notices
│   ├── LICENSE_INFORMATION.md      # GPL relicensing / pre-relicense MIT notice
│   ├── MIT-pre-relicense.txt       # MIT text for code predating the relicense
│   ├── linux/                      # Generated Linux THIRD_PARTY_NOTICES.md
│   ├── windows/                    # Generated Windows THIRD_PARTY_NOTICES.md
│   ├── macos/                      # Generated macOS THIRD_PARTY_NOTICES.md
│   └── manual/                     # Manual third-party notice fragments
├── user/                           # User-facing guides published by the website
│   ├── external-components.md      # External component setup guide
│   └── external-components.ja.md   # Japanese external component setup guide
├── download-verification.md        # Download verification guide
├── download-verification.ja.md     # Japanese download verification guide
└── internal/                       # Maintainer/internal operations docs
```

## Current Layout Notes

- The Japanese user-facing README lives at [`../README.ja.md`](../README.ja.md)
  beside the English root README. It is not a translation of this documentation
  index.
- Documentation directories and handwritten Markdown files should use
  lowercase kebab-case. Generated legal notice files named
  `THIRD_PARTY_NOTICES.md` are the current exception.
- `tmp/THIRD_PARTY_NOTICES.md` is tracked and used as the current Tauri bundled
  third-party notices resource. Although it lives under `tmp/`, it is not a
  disposable scratch file. Moving it to a clearer runtime resource location,
  such as `src-tauri/resources/THIRD_PARTY_NOTICES.md`, should be handled as a
  separate release/bundling change.

## Root-Level Documents

Some documents intentionally live at the repository root because GitHub or
contributors expect them there:

- [`README.md`](../README.md)
- [`CONTRIBUTING.md`](../CONTRIBUTING.md)
- [`SECURITY.md`](../SECURITY.md)
- [`CODE_SIGNING_POLICY.md`](../CODE_SIGNING_POLICY.md)
- [`LICENSE`](../LICENSE)
