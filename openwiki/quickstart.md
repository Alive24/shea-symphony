---
type: Quickstart
title: Shea Symphony code wiki
description: Entry point to Shea Symphony's architecture, lifecycle, operator workflows, source map, operations, testing, and 2607 hardening status.
tags: ["shea-symphony", "quickstart", "temporal", "orchestration"]
---

# Shea Symphony code wiki

Shea Symphony is a Rust orchestration system with a Svelte/Tauri operator cockpit for running AI-assisted engineering work through explicit Main, Review, Human Review, Rework, Merge, and recovery boundaries. It extends an OpenAI Symphony-style harness with tracker-backed team workflow, evidence, human gates, and Doctor diagnostics (`README.md`).

> **Authority and freshness:** this wiki is a derived navigation and synthesis layer. Source code, explicitly status-labelled repository documents, and the live tracker remain authoritative for their respective facts. Repository plans and issue references do **not** prove current GitHub progress.

## Current state at a glance

The repository is mid-migration:

- **2606 MVP** is the proven CLI/lane implementation and protected fallback. Its code remains in `src/commands/**`, `src/lanes/**`, and related legacy modules.
- **2607 Hardening** is a **Draft** milestone that replaces the hand-rolled durable loop with local Temporal while preserving 2606 workflow semantics (`docs/milestones/2607-hardening/README.md`).
- Current `src/main.rs` starts the 2607 Temporal worker host; it no longer dispatches the legacy CLI.
- The 2607 runtime has real worker/client scaffolding, pure Coordinator activation identity, and substantial SQLite foundations, but `IssueWorkflow` still runs a no-op Activity. Tracker transitions and agent/local Activities remain inert or placeholder implementations.
- The desktop app still reads most operational data and controls Autoloop through legacy CLI surfaces; its direct 2607 integration currently exposes read-only Temporal readiness.

See [Architecture](architecture/overview.md) for the migration shape and [Source map](source-map.md) for the precise implementation anchors.

## Major concepts

- [Architecture overview](architecture/overview.md) explains the Rust library/binary, Temporal workers, Tauri app, and the 2606-to-2607 boundary.
- [Authority and state](architecture/authority-and-state.md) distinguishes tracker business state, Temporal runtime authority, SQLite projections, and filesystem evidence.
- [2607 execution contracts](architecture/execution-contracts.md) separates the Draft T2607-05/06 target for Workflow state, agent Activities, leases, progress, failure routing, and lifecycle commits from the checked-in no-op skeleton.
- [2607 App and operator integration](architecture/app-operator-integration.md) contrasts the Draft T2607-07 SQLite/Temporal/operator-action target with the checked-in CLI/Autoloop coupling and read-only Temporal seam.
- [Lifecycle and domain](domain/lifecycle.md) defines executable versus static states and the meanings of NTC, NHI, Human Review, and lane ownership.
- [Operator workflows](workflows/operator-workflows.md) maps Main, Review, Merge, Doctor, and human handoff behavior.
- [Operations](operations.md) provides current-main and protected-2606 run guidance, configuration boundaries, and recovery cautions.
- [Testing](testing.md) maps deterministic tests, app checks, and opt-in live/Temporal smoke tests.
- [Source map](source-map.md) gives a practical reading order and records known documentation/code drift.

## Fast orientation

1. Read [Architecture overview](architecture/overview.md), then [Authority and state](architecture/authority-and-state.md); most incorrect changes come from assigning authority to the wrong layer. For T2607-05/06, continue to [2607 execution contracts](architecture/execution-contracts.md).
2. Before changing routing, read [Lifecycle and domain](domain/lifecycle.md) plus the relevant lane prompt under `.shea/prompts/`.
3. Before changing operator behavior, use [Operator workflows](workflows/operator-workflows.md) and the canonical existing specs it links instead of copying those specs.
4. Run the targeted checks in [Testing](testing.md). Temporal durable Workflow/Activity names and serialized DTO fields require compatibility planning once recorded in history (`src/symphony/mod.rs`).

## Build basics

```bash
cargo test
cargo build
npm --prefix app test
npm --prefix app run check
npm --prefix app run build
```

The default Rust binary expects a local Temporal service at the configured address (normally `localhost:7233`) and loads `.shea/workflows/shea-symphony.md`, overridable with `SHEA_WORKFLOW_PATH`. Running the operational 2606 CLI is a separate concern; see [Operations](operations.md) before following older `cargo run -- <command>` documentation on current `main`.

## Backlog

- **2608 Workflow Graph extension** — `docs/milestones/2608-workflow-graph-extension/`; deferred because 2607 explicitly prepares boundaries but does not implement the graph runtime.
- **Issue Forge, Dream, and Reflect internals** — `src/issue_forge.rs`, `skills/`; important Shea extensions, but initial coverage focuses on runtime hardening and operator recovery.
- **Detailed tracker adapter matrix** — `src/tracker/**`; initial pages document authority and integration points rather than every backend capability.
- **Artifact retention and cleanup internals** — `docs/artifact-storage-policy.md`, `src/artifacts.rs`; deferred beyond the operational evidence-preservation rules.
