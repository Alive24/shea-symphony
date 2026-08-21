---
type: Source Map
title: Repository source and documentation map
description: Practical navigation map for Shea Symphony runtime code, operator app, workflow contracts, milestone design, tests, and known source/document drift.
tags: ["source-map", "navigation", "documentation", "implementation"]
---

# Repository source and documentation map

Start with the concept page matching the question, then use these anchors to verify details. Explicit document statuses are part of the evidence.

## Runtime implementation

| Area | Primary sources | Notes |
| --- | --- | --- |
| Current binary | `src/main.rs` | 2607 Temporal worker host, not legacy CLI dispatcher |
| Shared boundary | `src/lib.rs`, `src/symphony/mod.rs` | exports legacy 2606 modules plus 2607 contracts |
| Temporal client/runtime | `src/symphony/client.rs`, `worker_runtime.rs`, `workers.rs`, `task_queues.rs` | real connection and registration scaffolding |
| Workflow | `src/symphony/workflows.rs`, `dto.rs` | current no-op skeleton and bounded Query state |
| Activities | `src/symphony/activities.rs` | no-op health paths plus inert/placeholder product Activities |
| Coordinator | `src/symphony/coordinator/mod.rs` | implemented pure activation and Workflow identity only |
| SQLite | `src/symphony/local_state/**` | migration/admin/projector/reader foundations; not App/Coordinator-wired |
| Legacy implementation on main | `src/commands/**`, `src/lanes/**`, `src/review.rs`, `src/merge_lane.rs` | primarily reference/acceptance evidence, not the active protected binary source or 2607 target architecture; bounded protocol-independent components may be reused after ownership review when they fit new typed boundaries and focused tests |
| Tracker adapters | `src/tracker.rs`, `src/tracker/**` | 2606 external-state access and mutation capabilities |
| Doctor | `src/commands/doctor.rs`, `src/doctor/**` | diagnostics, topology, evidence gates, bounded repairs |

Use [Architecture overview](architecture/overview.md) to understand how these pieces coexist, [Protected 2606 bootstrap runtime](architecture/2606-bootstrap-runtime.md) for the current self-hosting engine/target topology and retirement criteria, [Authority and state](architecture/authority-and-state.md) before interpreting storage code, and [2607 execution contracts](architecture/execution-contracts.md) when implementing the T2607-05/06 Workflow and Activity boundaries.

## Desktop application

- `app/src/OperatorDesk.svelte` — operator cockpit and Human Todo composition.
- `app/src/lib/**` — stores, view models, handoff rendering, and UI models.
- `app/src-tauri/src/read_surfaces.rs` — legacy CLI-backed reads.
- `app/src-tauri/src/autoloop.rs` — legacy Autoloop child-process control.
- `app/src-tauri/src/temporal_health.rs` — direct read-only 2607 readiness.
- `app/test/**` — Node tests for operator behavior and optional static build.

The [Operator workflows](workflows/operator-workflows.md) page connects these surfaces to lane and human authority. [2607 App and operator integration](architecture/app-operator-integration.md) maps the Draft replacement boundary and its unresolved static-handoff action bootstrap. **Tracking:** `OPERATOR-ACTION-BRIDGE.md` plus T2607-07 own that follow-up; no Issue is promoted, and #505 is only a bounded backend Backlog seed excluding Svelte UX.

## Workflow-owned files under `.shea`

Only these trees are in this wiki's inspected scope:

- `.shea/workflows/shea-symphony.md` — canonical checked-in tracker/runtime/lane configuration.
- `.shea/prompts/**` — Main, Review, Merge, and human handoff contracts.
- `.shea/template/**` — workpad/evidence templates.

Other `.shea` runtime, artifact, log, worktree, local, app, and binary trees are intentionally excluded from wiki discovery.

## Canonical design/status reading order

1. `docs/milestones/2607-hardening/README.md` — **Draft** milestone scope and 2606 relationship.
2. `TEMPORAL-SPINE.md` and ADR 0006 — **Draft/Proposed** runtime target.
3. `WORKFLOW-ACTIVATION.md` — **Draft** episode and reconciliation contract; pure identity slice is implemented.
4. `LOCAL-STATE-DB.md` — **Draft** detailed contract; ADR 0007 is **Accepted** and several slices are implemented.
5. `implementation/T2607-04-tracker-transition-activity.md` — tracker commit mechanics and the partially implemented transition contract.
6. `ISSUE-WORKFLOW-STATE.md`, `AGENT-ACTIVITY-CONTRACT.md`, `ACTIVITY-ERROR-TAXONOMY.md`, `TEMPORAL-CONCURRENCY.md`, `TASK-QUEUES.md`, `ISSUE-WORKFLOW.md`, and `RUNTIME-ROLE-MAPPING.md` — **Draft** execution contracts synthesized in [2607 execution contracts](architecture/execution-contracts.md), not current behavior.
7. implementation T2607-05 and `T2607-06-issue-workflow-state-machine.md` — **Draft** packages for agent Activities and workflow routing; registration or DTO seams do not satisfy them.
8. `implementation/README.md` and T2607-01 through T2607-08 — dependency-aware packages, not proof of live tracker status. Read `STATUS.md` and the configured Project for current progress.

For 2606 behavior, start from `docs/README.md`, the selected repository Skill,
`.shea/contracts/workflow-capability.v1.md`, its legacy adapter, and current
legacy source/tests. Treat it as an internal compatibility, recovery, and
acceptance baseline rather than the public user entrypoint.

## Tests

- `tests/temporal_noop_smoke.rs` — opt-in Temporal skeleton smoke.
- `tests/live_github_smoke.rs`, `tests/live_linear_smoke.rs` — opt-in read-only integration smoke.
- `tests/parent_subissue_topology.rs` — relationship/branch topology.
- `src/main/tests/**`, `src/doctor/tests/**`, module-local tests — 2606 workflow and diagnostics.
- `app/test/**`, Tauri module tests — UI and backend bridge.

See [Testing](testing.md) for commands and limits.

## Recent history with architectural significance

- Coordinator activation/identity landed as a pure contract before launcher integration.
- Active SQLite reading landed without lifecycle inference, reinforcing projection boundaries.
- Tracker transition DTO/idempotency landed while keeping the Activity explicitly inert.
- Human handoff text moved into repository prompt files, making operator policy versioned and testable.
- A recent review fix addressed false active-worker detection, emphasizing that claims/runtime evidence must be interpreted conservatively.

History helps explain intent, but current source and explicit status still control claims.

## Known drift and conflicts

- README/CLI/operator docs describe the complete 2606 product runtime, while current `src/main.rs` starts partial 2607 workers. The operational bridge is the protected-2606-built vendored App/CLI bootstrap against canonical main, not current main's root binary.
- The desktop's legacy Cargo fallback can point at a root binary that is now Temporal-only, so it is not equivalent to selecting the protected bootstrap CLI.
- ADR 0006 is Proposed despite partial implementation; do not relabel it Accepted.
- Planned 2607 App, tracker write, agent Activity, and state-machine behavior exceeds current implementation. **Tracking:** T2607-07 owns App product integration, remaining tracker work belongs to T2607-04, and agent/state-machine behavior belongs to T2607-05/06; none has a promoted Issue. #505 is only a bounded Coordinator/App backend Backlog seed excluding Svelte UX.
- SQLite schema can represent lifecycle spellings the current projector intentionally never writes; this is an intentional v1 projector boundary under ADR 0007 rather than an implementation-status claim.
- Doctor dirty-merge guidance conflicts with the newer merge policy established by Done issue #390. **Tracking:** retained 2606 Doctor repair is an unowned gap or removable under T2607-08, for which no Issue is promoted.
- Parent/subissue docs contain some future-tense language for checks now present in code/tests. **Tracking:** unowned documentation drift.

When a conflict affects a change, report it and cite both sides; do not resolve it implicitly. [Operations](operations.md) provides the practical runtime warning.
