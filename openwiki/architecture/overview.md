---
type: Architecture Overview
title: Runtime architecture and 2606/2607 boundary
description: Current Shea Symphony architecture, including the legacy 2606 lane runtime, the partial 2607 Temporal spine, worker queues, and desktop integration.
tags: ["architecture", "temporal", "rust", "tauri", "2607"]
---

# Runtime architecture and 2606/2607 boundary

## Two generations, one migration

The reusable crate currently exposes both generations: legacy modules remain in current main while `src/symphony/**` establishes the 2607 Temporal execution boundary (`src/lib.rs`). Operationally, however, the complete workflow comes from the [protected 2606 bootstrap runtime](2606-bootstrap-runtime.md): protected-branch-built vendored App and CLI binaries operate against canonical main while 2607 is incomplete. [Lifecycle semantics](../domain/lifecycle.md) come from that proven workflow, but 2607 must re-express them through new contracts and retire—not wrap—the Autoloop, product CLI, vendored-runtime assumptions, and legacy orchestration and authority implementation. This still permits deliberate, reviewed reuse of bounded Rust components when they fit the new typed ownership boundaries and have focused tests.

2607 Hardening is explicitly **Draft** (`docs/milestones/2607-hardening/README.md`). ADR 0006, which proposes local Temporal as the runtime spine, remains **Proposed** even though current `src/main.rs` and `src/symphony/**` already realize parts of it. Preserve those statuses; implementation does not silently accept an ADR.

```mermaid
flowchart TD
    App["Svelte operator cockpit"] --> Tauri["Tauri Rust commands"]
    Tauri --> Legacy["2606 CLI read and Autoloop surfaces"]
    Tauri --> Health["2607 Temporal readiness"]
    Main["Current Rust binary"] --> Host["Temporal worker host"]
    Host --> Core["symphony-core"]
    Host --> AgentQ["symphony-agent"]
    Host --> LocalQ["symphony-local"]
    Core --> IW["IssueWorkflow no-op skeleton"]
    AgentQ --> Placeholder["Agent Activity placeholders"]
    LocalQ --> LocalPlaceholder["Local Activity placeholders"]
    Legacy --> Tracker["Tracker and 2606 lane runtime"]
```

This shows the checked-in current-main integration: the app remains mostly 2606-facing while the default binary hosts the partial 2607 worker topology.

## 2607 component responsibilities

### Temporal client and worker host

- `src/main.rs` loads `.shea/workflows/shea-symphony.md` and calls `run_symphony_workers`.
- `src/symphony/client.rs` provides service readiness, Workflow start, query, and result retrieval.
- `src/symphony/worker_runtime.rs` starts core, agent, and local workers in one process.
- `src/symphony/workers.rs` registers durable Workflow and Activity names.

The queues separate latency-sensitive orchestration from expensive agents and short local projection work:

| Queue | Intended ownership | Checked-in implementation |
| --- | --- | --- |
| `symphony-core` | `IssueWorkflow`, tracker commits, control plane | Workflow plus successful no-op and inert tracker transition |
| `symphony-agent` | Main, Rework, Review, Merge attempts | Registered Activities returning `not_implemented` |
| `symphony-local` | projection, indexing, local health | projection/index placeholders; local health succeeds without writes |

The workflow config declares concurrency 3/3/8. `task_queue_registrations` reports these limits, but the inspected `WorkerOptions` builders do not visibly apply them; treat enforcement as unproven rather than implemented. **Tracking:** worker concurrency enforcement is an unowned T2607-01 residual that must be settled before T2607-05/06 rely on it.

### IssueWorkflow

[Authority and state](authority-and-state.md) places deterministic in-flight decisions in `IssueWorkflow`; all I/O must cross an Activity boundary. Current `src/symphony/workflows.rs` only schedules `NoopCoreActivity`, records `completed_noop`, and exposes a `current_state` Query. It does **not** yet implement lane routing, signals/updates, tracker transitions, Human Review, NHI resumption, or the proposed durable state machine in `docs/milestones/2607-hardening/ISSUE-WORKFLOW.md` (**Draft**).

### Workflow Coordinator

`src/symphony/coordinator/mod.rs` implements the pure portion of [workflow activation](authority-and-state.md): classify an observed tracker state, enforce optional optimistic expectations, derive `work`, `review`, `rework`, or `merge`, and build a bounded episode Workflow ID from explicit inputs. It performs no I/O. Production tracker observation, Temporal start/Describe, capacity admission, and projection wiring remain absent. **Tracking (verified 2026-07-28):** #502 is Todo and is the next T2607-03 Temporal-authoritative start slice; #504 is a Backlog seed for stale Coordinator-binding repair, and #505 is a Backlog seed for the bounded real caller/App backend. Capacity admission is an unowned T2607-03 gap. #503 is an older Backlog start seed now overlapped by #502, not another implementation slice. T2607-04 separately owns tracker mutation, transition evidence, and tracker-side reconciliation, with no promoted Issue.

### Local SQLite

The local database is the most mature 2607 subsystem. ADR 0007 is **Accepted**. `src/symphony/local_state/**` implements schema/migration, typed identity, health/admin, Describe-backed projection, and narrow active-row reads. It is not yet connected to Coordinator or the App; see [Authority and state](authority-and-state.md). **Tracking (verified 2026-07-28):** Coordinator/Describe projection wiring belongs to T2607-03; #502 is Todo next and #504 is its Backlog repair seed. App wiring belongs to T2607-07, which has no promoted Issue; #505 is only a bounded backend Backlog seed.

### Desktop app

`app/src/OperatorDesk.svelte` and `app/src/lib/**` present queues, Human Todo, lane evidence, and handoffs. Tauri commands in `app/src-tauri/src/read_surfaces.rs` still shell out to legacy CLI JSON surfaces, while `app/src-tauri/src/autoloop.rs` controls the 2606 loop. `app/src-tauri/src/temporal_health.rs` is the direct 2607 seam: a bounded, read-only readiness probe through the shared client. [2607 App and operator integration](app-operator-integration.md) contrasts this implementation with the Draft T2607-07 dashboard, direct Temporal, and routed-action target.

## 2606 relationship

The [protected 2606 bootstrap runtime](2606-bootstrap-runtime.md) has three separate current roles: active App/CLI development bootstrap against canonical main, protected recovery baseline, and behavior/test/evidence acceptance oracle. Current main is the forward-development target and its root binary already hosts the partial 2607 worker topology; it is not yet the source of the complete operational product CLI.

2607 preserves required behavior while replacing implementation and ownership: [tracker commits become Activities](authority-and-state.md), executable progression becomes Workflow state, top-level reads become local projections, and operator mutations use narrow Tauri/action contracts. Acceptance may be defined by 2606 tests and evidence, but invoking or wrapping old lane/CLI code does not satisfy the target architecture.

## Change guidance

- Changing Workflow/Activity names or DTO serialization requires a Temporal history compatibility plan.
- Do not put tracker, network, filesystem, SQLite, process, or clock I/O in Workflow code.
- Do not interpret placeholder registration as feature completion.
- Keep the desktop and legacy CLI coupling visible until T2607-07 is actually implemented.
- Use [Testing](../testing.md) for queue registration, no-op smoke, app, and migration checks.
