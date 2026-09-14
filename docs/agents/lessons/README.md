# AI Learning Records

This directory records evidence-backed lessons from maintainer corrections,
repeated failures, surprising invariants, and costly investigations. It is not
an automatically trusted source of product truth.

The workflow is:

```text
candidate lesson
  -> confirm against current code, docs, tests, logs, or runtime data
  -> promote the durable part to the correct canonical surface
  -> add a deterministic test, script, hook, or CI check when possible
  -> keep this record as provenance and a revalidation trigger
```

Use the
[`capture-project-learning`](../../../.agents/skills/capture-project-learning/SKILL.md)
skill to add or promote a record.

## Status

- `candidate`: not yet promoted; its cause can still be a hypothesis or already
  confirmed.
- `promoted`: confirmed and represented in a canonical rule, document, skill,
  test, hook, or CI check.
- `superseded`: retained for history but replaced by a newer record or design.

`cause_status` is separate from promotion status:

- `hypothesis`: the current explanation still needs evidence.
- `confirmed`: current evidence supports the stated root cause.

## Required Frontmatter

Each lesson is one Markdown file with this frontmatter:

```yaml
---
id: LRN-YYYYMMDD-short-slug
status: candidate | promoted | superseded
cause_status: hypothesis | confirmed
scope: paths or workflow
trigger: when an agent should search for this lesson
failure_signature: the observed correction or failure
root_cause: confirmed cause or clearly labeled current hypothesis
guardrail: canonical destination or pending action
canonical_refs: promoted owners, or pending for a candidate
verification: exact assertion, evidence path, or command
evidence: repository paths, tests, logs, issue, PR, or commit
revalidate_when: version, architecture, or behavior condition
superseded_by: replacement LRN ID; required only when status is superseded
---
```

Do not record secrets, credentials, personal absolute paths, guesses presented
as facts, or a temporary PR/check state as a durable rule. Time-specific
versions and IDs may appear as historical evidence only when the record also
states when to revalidate them.

For a promoted lesson, `canonical_refs` is a comma-separated list of
repository-relative paths that must exist. A superseded lesson points
`superseded_by` to another lesson's `LRN-...` ID.

## Promotion Destinations

| Learning type | Canonical destination |
| --- | --- |
| Product vocabulary | `CONTEXT.md` |
| Cross-cutting design lens | `docs/design-principles.md` |
| Specific trade-off | `docs/adr/**` |
| Current ownership or structure | `docs/architecture/**` or owner README |
| Always-on AI rule | `AGENTS.md` |
| Path-specific AI rule | `.agents/rules/**` or nested `AGENTS.md` |
| Repeated multi-step workflow | `.agents/skills/**` |
| Deterministic invariant | Test, script, hook, or CI |
| Environment-specific or expiring fact | This directory only |

## Validation Surfaces

- `PreToolUse` blocks direct edits to generated bindings when the target path
  can be determined safely.
- `PostToolUse` validates only the touched guidance file's local syntax and
  schema. Cross-file checks are deferred so a new lesson and its index can be
  edited in separate tool calls.
- `Stop` runs the complete cross-file validator and reports unresolved drift
  before the agent finishes.
- `.github/workflows/agent-guidance.yml` runs the same deterministic checks in
  a path-filtered workflow. It stays separate from product CI so guidance-only
  failures have a clear owner and do not pay the application build matrix cost.

The hooks resolve the repository from Git rather than assuming they were
started at the repository root. All checks are non-mutating; CI remains the
shared enforcement surface.

## Records

- [Evidence before conclusions](evidence-before-conclusions.md)
- [Keep agent guidance aligned with canonical architecture](guidance-follows-canonical-architecture.md)
- [Persist application preferences through settings](persist-application-preferences-through-settings.md)
- [Legacy GPU source display preference](legacy-gpu-source-display-preference.md)
- [Separate environment failures from product regressions](separate-environment-failures.md)
- [Verify user-visible results](verify-user-visible-results.md)
- [Preserve clean-room specification gates](preserve-clean-room-spec-gates.md)
- [Prefer experimental sensor coverage](prefer-experimental-sensor-coverage.md)
- [Preserve maintainer direction during AI review](preserve-maintainer-direction-during-ai-review.md)
- [Prevent Tauri dependency version skew](prevent-tauri-dependency-version-skew.md)
- [Keep shared rules agent-neutral](keep-shared-rules-agent-neutral.md)
- [Make navigation levels explicit](make-navigation-levels-explicit.md)
- [Separate release integrity from vulnerability exposure](separate-release-integrity-from-vulnerability-exposure.md)
- [Deliver pull requests through review](deliver-pull-requests-through-review.md)
- [Keep required CodeQL configurations present on pull requests](keep-required-codeql-configurations-on-pull-requests.md)
- [Keep shared system refresh ownership explicit](keep-shared-system-refresh-ownership-explicit.md)
- [Keep CPU power sampling independent](keep-independent-cpu-power-sampler.md)
- [Read id producers before identity UI](read-id-producers-before-identity-ui.md)
- [Bound bot review loops](bound-bot-review-loops.md)
- [WebView2 suspension requires hiding the controller](webview2-suspend-requires-hidden-controller.md)
- [Stabilize performance memory baselines](stabilize-perf-memory-baselines.md)
- [Separate sensor support from recording coverage](separate-sensor-support-from-recording-coverage.md)
- [Keep Design Docs focused on decisions](keep-design-docs-focused-on-decisions.md)
- [Report across the elevation boundary by exit code](report-across-elevation-by-exit-code.md)
