# HardwareVisualizer Design Principles

This document is the default decision lens for product, architecture, and
implementation work in HardwareVisualizer. It records stable direction rather
than a feature list or a copy of the current file tree.

A specific accepted ADR can refine one of these principles. If an ADR, the
current implementation, and this document disagree, do not silently choose one:
surface the drift and update the appropriate source of truth after the decision
is confirmed.

## Product Principles

### DP-01: Keep hardware data locally owned

HardwareVisualizer observes a local machine for the person using that machine.
Settings, history, and hardware identity data stay local by default, and the app
does not send outbound telemetry. Persist only the identifying material needed
for a product capability and avoid turning local identifiers into portable user
or device identities.

This is local data ownership, not a promise that the application never uses the
network. Release updates and user-initiated links are separate product
capabilities.

See [README](../README.md#permissions--security-notes) and the proposed
[ADR 0003](adr/0003-storage-health-device-identity.md).

### DP-02: Represent partial capability honestly

Useful partial data is better than an all-or-nothing response. Preserve the
availability and validity distinctions defined by the target domain; for
example, sensor availability, fan activity/validity, and storage freshness are
different models rather than one universal status enum. Never turn a missing
reading into zero, healthy, disconnected, or a whole-device failure without
evidence.

For vendor- and OS-dependent metrics, return the values that are available and
carry source or unavailability information when it helps the user understand
the result. HardwareVisualizer reports observations and sampled summaries; it
must not overclaim a complete process audit, authoritative fault diagnosis, or
proof of hardware health from one reading.

For native sensors, prefer a best-effort experimental attempt over a hard
allowlist when an existing read-only path recognizes the hardware and an
existing plausibility-gated decode can be attempted without inventing an
address, register map, or chip selection. Maintain verification status in the
sensor specifications rather than routine UI readings. If an experimental
attempt fails and an existing diagnostic is surfaced, that failure may identify
the path as experimental. A specification may back such an experimental scope
with facts learned from a copyleft monitoring implementation, tagged lead-only;
verified scopes still need a primary source or an accepted hardware dump. See
[ADR 0011](adr/0011-experimental-sensor-enablement.md) and
[ADR 0023](adr/0023-copyleft-derived-facts-for-experimental-scopes.md).

See [Backend Architecture](architecture/backend.md#add-a-new-hardware-data-source)
and [Product Vocabulary](../CONTEXT.md#sensor-availability).

### DP-03: Optional components enhance; they do not gate

An optional driver, tool, or vendor library may unlock more data, but it is not
an application startup requirement. Show External Component Guidance only after
the component was actually attempted, could not be used, and fallback
collection still leaves important user-visible data unavailable.

Unsupported hardware is not an installation failure. Diagnostic guidance must
not change collection results, fallback order, or aggregate success behavior.
Never install an external component or elevate the process silently; component
setup and privilege elevation are explicit, capability-scoped user choices.

See [Backend Architecture](architecture/backend.md#add-a-new-hardware-data-source)
and [External Components](user/external-components.md).

### DP-04: Make work follow visible value

The monitor must not become the workload. Continuous collection, rendering,
and fan-out must have a user or background-product purpose. Gate window-only
delivery when the window is hidden or minimized, avoid redundant work in
measured hot paths, and collect expensive data on demand when it is only useful
in a visible view. This principle does not claim that every current event path
already deduplicates same-value updates.

Do not generalize this into "stop everything when hidden." Hardware Archive,
the Tray Widget, alerts, and other explicit background behaviors can justify
continued work.

See [ADR 0006](adr/0006-live-storage-health-on-demand.md) and
[Backend Architecture](architecture/backend.md#core-collector-coresrccollector).

### DP-05: Treat time and subject as product semantics

Current readings, a short-window Usage Graph, the Hardware Archive, a daily
Storage Health Record, and Live Storage Health have different meanings even
when their fields look similar. Keep their collection cadence, retention,
freshness, and UI language distinct.

Likewise, distinguish the current device, Focus Storage Device, and Selected
Storage Device. A transient failure or empty enumeration is uncertainty, not
proof that a device was removed.

See [Product Vocabulary](../CONTEXT.md),
[ADR 0004](adr/0004-separate-storage-health-history.md), and
[ADR 0006](adr/0006-live-storage-health-on-demand.md).

### DP-06: Preserve explicit user intent

Automatic selection and heuristics may choose a useful default, but they must
not silently fight an explicit user choice while that choice remains valid. If
the selected subject is temporarily unavailable, fall back coherently without
discarding the stored intent unnecessarily, so it can be restored when valid
again. Surface risk through a separate signal instead of mixing aggregate and
selected-subject semantics.

Persist choices that users reasonably understand as application configuration.
Use transient storage only for state that can be reset without losing an
explicit preference.

See [Product Vocabulary](../CONTEXT.md#preferences-and-app-behavior), the
proposed Storage Health specialization in
[ADR 0008](adr/0008-selected-storage-device-overrides-focus.md), and
[Backend Architecture](architecture/backend.md#settings-ownership).

## Engineering Principles

### DP-07: Make ownership follow runtime dependencies

Core owns Tauri-independent facts and behavior: collection, platform access,
raw models, persistence, and Core-consumed settings. The App crate owns Tauri
lifecycle, IPC, wire DTOs, presentation conversion, plugins, and App-owned
settings. The frontend owns interaction and view state.

Core publishes data through its EventBus; App adapters decide how to expose it
to Tauri. Commands stay thin. OS-specific access remains behind Core platform
traits, the PlatformFactory, and providers.

See [ADR 0002](adr/0002-core-app-split.md) and
[Backend Architecture](architecture/backend.md).

### DP-08: Keep language and boundaries explicit

Name user concepts by their product meaning, not by the provider that happens
to implement them. `CONTEXT.md` owns vocabulary, ADRs own specific decisions,
and architecture documents own the current structural explanation.

Use typed IPC and generated boundaries. Do not hand-edit generated TypeScript
bindings or generated App DTOs. Change the owning Rust model or command,
regenerate, and preserve unknown settings keys owned by the other crate.

See [Product Vocabulary](../CONTEXT.md),
[ADR 0005](adr/0005-storage-health-naming.md), and
[ADR 0009](adr/0009-generated-app-hardware-dtos.md).

### DP-09: Match evidence to the claim

Use the narrowest evidence that can support the scoped claim when inspected:

- code inspection identifies a possible path;
- a unit or integration test proves a contract in its test environment;
- runtime logs and SQLite rows support scoped runtime/persistence claims;
- web/mock E2E supports deterministic frontend behavior;
- native E2E supports Tauri integration, not every hardware provider;
- a rendered screenshot supports only the inspected frame and environment;
- measured job timings and performance harnesses support performance claims.

For CI failures, inspect the failing leaf job and its exact error before
changing code. Treat chat memory, handoff notes, aggregate checks, and stale
checkouts as leads, not proof.

See [E2E Capture Harness](development/e2e-captures.md) and
[AI Learning Records](agents/lessons/README.md).

### DP-10: Deliver one coherent product claim at a time

Use the issue or explicit request as the scope anchor. Keep adjacent features,
cleanup, and governance changes separate unless they are required for the same
behavioral claim. Prefer a narrow vertical slice with a visible or testable
outcome over a broad layer-by-layer rewrite.

Verification should scale with risk: start with the smallest relevant check,
then expand when the change crosses ownership or user-facing boundaries.

### DP-11: Treat trust and licensing as product behavior

Distribution provenance, signing status, local privacy, and clean-room sensor
development affect what the product can honestly claim. Do not bypass a
security, licensing, provenance, or implementation-readiness gate to accelerate
delivery.

For PawnIO CPU or Super I/O work, the clean-room specification and role rules
are mandatory inputs, not optional documentation. Access remains read-only: do
not write chip configuration, fan-control, limit, or power-state registers.

See [Clean-room Sensor Specifications](specs/sensors/README.md),
[Code Signing Policy](../CODE_SIGNING_POLICY.md), and
[Download Verification](download-verification.md).

## Decision Checklist

Before implementing or approving a design-changing change, answer:

1. Which product principle and user-visible claim does this change serve?
2. Who owns the fact, policy, presentation, lifecycle, and persisted value?
3. Is the value live, short-window, archived, daily, or UI-local?
4. What happens when one metric, provider, device, or permission is missing?
5. Does a valid explicit choice survive automatic refresh, and does it survive
   restart only when classified as an Application Preference?
6. Does the collection or rendering cost follow actual visible/background value?
7. Is terminology already defined in `CONTEXT.md`?
8. What evidence would prove the intended claim, including the rendered result?
9. Does the decision require an ADR, a regression test, or a clean-room gate?
10. Is any adjacent work better kept in a separate issue or PR?

## Source-of-Truth Map

- Security, licensing, and clean-room gates are non-negotiable constraints.
- An accepted ADR owns a specific trade-off and its consequences.
- This document owns the cross-cutting decision lens.
- `docs/architecture/**` and owner README files describe the current structure.
- `CONTEXT.md` owns product terms and avoided aliases.
- Current code, tests, runtime data, and GitHub state establish current facts.
- `docs/agents/lessons/**`, handoffs, chat, and AI memory are evidence leads;
  they never override current canonical sources without verification.
