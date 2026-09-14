---
id: LRN-20260914-report-across-elevation-by-exit-code
status: promoted
cause_status: confirmed
scope: any code path where an elevated child process reports back to the unelevated app, or stages files it later executes
trigger: designing a result channel, staging directory, or file placement for an elevated helper, installer step, or External Component Setup
failure_signature: the first External Component Setup design passed a result-file path in the user's temp directory to the elevated child and staged the verified installer there; review identified elevated arbitrary-write, result forgery, and post-verification swap by a same-user medium-integrity process
root_cause: paths the unelevated caller controls are writable by every medium-integrity process of that user, so an elevated child that writes to, or re-opens and executes from, such a path trusts an attacker-controllable location
guardrail: ADR 0024 decision 3 and the module docs of core/src/external_component_setup; the elevated setup child reports only through its exit code, stages under an administrator-only directory, and never treats an unreadable state as absence
canonical_refs: docs/adr/0024-external-component-setup.md, core/src/external_component_setup/mod.rs, core/src/external_component_setup/windows.rs
verification: cargo test -p hardviz-core external_component_setup (exit-code round trip, unknown-state blocking) and the Windows CI compile of the staging and registry paths
evidence: "PR #2120 review threads by Codex and CodeRabbit on the result file and staging directory, and the correction commits on that PR"
revalidate_when: a second elevated helper or service is introduced, or the setup child needs to return more than an outcome and stage
---

# Report Across The Elevation Boundary By Exit Code

An elevated child must not write to, nor execute from, a path its unelevated
caller chose. Every medium-integrity process of the same user can replace such
a path with a link or swap the file after verification. Return the outcome
through the exit code of the process handle the caller owns, encode the failing
stage in that code when the UI needs an explanation, and stage anything the
child will execute under an administrator-only directory it created itself.

Treat an unreadable registry key or directory as unknown state that blocks the
action, not as evidence of absence, and publish files atomically with a
no-clobber link so a partial or concurrent write never becomes the final file.
