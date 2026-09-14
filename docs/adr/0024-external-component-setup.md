# External Component Setup

Status: proposed

HardwareVisualizer can collect deeper Windows sensor data through PawnIO, but
until now the user had to find the PawnIO runtime installer and the separate
signed module blobs, install them by hand, and place the module files under
the PawnIO install directory. External Component Guidance explains that
process, yet it still leaves the most valuable Windows readings (CPU package
temperature and power, motherboard temperatures and fans) unavailable for most
users.

We decided to add **External Component Setup**: an explicit, user-initiated
action in which HardwareVisualizer downloads a pinned upstream release of an
optional component, verifies it, and installs it with the user's elevation
consent. The first component is PawnIO on Windows. The vocabulary and
mechanism are component-neutral so that other optional components can be added
later without a second design.

## Decision

1. **Setup is an explicit user choice on every channel.** The Windows
   installers will offer External Component Setup as a per-component option
   that is selected by default in an interactive install and can be
   deselected (planned in #2118). The Settings screen offers the same setup
   at any later time (the first implemented entry point). A silent or
   unattended install (`msiexec /qn`, NSIS `/S`, package managers) runs no
   setup unless the caller passes the documented property or flag, because
   nobody could see or decline the option. This keeps DP-03: an optional
   component is never installed silently.
2. **Artifacts are downloaded at setup time from the upstream release, pinned
   by version and SHA-256.** HardwareVisualizer does not redistribute the
   PawnIO runtime installer or the module blobs inside its own installers. The
   pinned URL, size, and digest live in Core; a download whose digest does not
   match is discarded. The pinned versions are bumped through ordinary
   dependency-style pull requests, and the module release follows the tag the
   sensor specification was verified against.
3. **One executor for every entry point.** The application binary gains a
   command-line mode that runs the setup plan for one component and reports
   through its exit code only: a result file or pipe in a user-writable
   location would let a same-user medium-integrity process redirect an
   elevated write or forge the result. The Settings action launches that
   mode elevated through the existing Windows elevation path and waits for
   it; the planned installer custom action invokes the same mode from its
   already elevated context. Core owns
   the plan, download, verification, and OS-level install steps behind the
   platform boundary; App owns the command-line dispatch, IPC, and UI.
4. **Setup fills gaps and never removes.** The runtime installer runs only
   when the runtime is positively known to be absent; an unreadable registry
   or directory is reported as unknown state and blocks setup. Module files
   are placed only when missing, atomically and without ever replacing an
   existing file. Uninstalling HardwareVisualizer never uninstalls PawnIO or
   deletes module files; an interactive uninstall will only tell the user
   that the component was kept (planned in #2119).
5. **A restart applies the result.** The PawnIO provider probes and caches its
   availability once per process, so the Settings flow tells the user to
   restart HardwareVisualizer after a successful setup rather than pretending
   the new readings are live.

## Alternatives Considered

- **Bundle the runtime installer and modules inside the MSI/NSIS package.**
  Rejected. The PawnIO runtime installer is distributed as proprietary
  freeware with no explicit redistribution grant, the modules are LGPL-2.1 and
  would need notices and a source offer, and bundling ties a kernel-driver
  version to every HardwareVisualizer release. Downloading pinned artifacts is
  the same approach the WebView2 bootstrapper already uses in this project.
- **A separate elevated helper or Windows service.** Rejected for the same
  reason as ADR 0007: it would add a process boundary, an IPC contract, and
  installer lifecycle work that the current requirement does not need.
- **Only a first-launch prompt instead of an installer option.** Rejected as
  the primary mechanism because the request is an installer-time option with
  opt-out. A first-launch prompt may still be added later for channels that
  install silently; it is recorded as an open question in the design doc.
- **Auto-install on silent installs because the option is "on by default".**
  Rejected. Default-on describes the interactive checkbox, not consent that a
  package manager can give on the user's behalf.

## Consequences

- HardwareVisualizer would use the network for a user-initiated component
  download in addition to release updates and user-opened links. This remains
  within DP-01: no hardware or usage data leaves the machine.
- The user documentation and the Windows external component checklist stop
  saying the app never installs components; they describe the pinned,
  verified, explicit setup instead.
- The Windows installers would gain custom dialogs and custom actions that
  must be verified on a Windows machine each time the Tauri bundler templates
  change.
- Setup failure never fails the HardwareVisualizer installation and never
  changes collection results; the app keeps its existing fallbacks and
  External Component Guidance.
- Detailed slices, installer mechanics, and open questions are recorded in
  [`docs/design/external-component-setup.md`](../design/external-component-setup.md).
