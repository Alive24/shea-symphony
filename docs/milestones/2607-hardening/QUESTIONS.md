# 2607 Hardening Open Decisions

Status: Active decision queue

Authority: Unresolved 2607 design decisions only

Scope: Decision gaps that can still change a 2607 contract. Resolved decisions
belong in their owning ADR or technical document; live delivery state belongs
in GitHub Project #9 and `STATUS.md`.

## Current Posture

No question below changes the accepted direction that Temporal is the runtime
spine, SQLite is the local read model, tracker writes use the transition
boundary, the App is the product surface, and the CLI is an admin/development
fallback. Coding agents should load this file only when working on an affected
context named below.

| ID | Decision gap | Affected context | Decide before |
| --- | --- | --- | --- |
| `Q2607-01` | What exact owner/repository/profile namespace should the installed machine-local runtime root use? | `WORKSPACE-CONFIG.md`, `implementation/T2607-01-temporal-runtime-skeleton.md`, `implementation/T2607-07-app-integration.md`, `docs/runtime-profiles.md` | Declaring the installed runtime and multi-workspace path contract stable. |
| `Q2607-02` | Does the current `.shea/workflows/shea-symphony.md` contract replace a legacy root `WORKFLOW.md`, or must onboarding support a bounded compatibility import? | `WORKSPACE-CONFIG.md`, `.agents/skills/setup-shea/SKILL.md`, `.shea/contracts/workflow-capability.v1.md` | Freezing onboarding migration and compatibility behavior. |
| `Q2607-03` | What measured time budgets should `project state` and an ordinary App refresh meet? | `PERFORMANCE.md`, `implementation/T2607-08-deletion-performance-hardening.md` | Accepting the first performance-hardening implementation slice. |
| `Q2607-04` | Should timing evidence be recorded in JSONL, status snapshots, or both? | `PERFORMANCE.md`, `AGENT-ACTIVITY-CONTRACT.md`, `docs/artifact-storage-policy.md` | Freezing the first timing-event schema. |

## Resolution Protocol

- Resolve a question in its owning ADR or technical contract, then remove its
  row here; do not retain an answered discussion log.
- Move a question to the 2608 milestone when it no longer changes a 2607
  contract. Workflow Graph layout and extension-output schema already live in
  `../2608-workflow-graph-extension/README.md`.
- Do not copy issue status, implementation progress, or historical debate into
  this file.
