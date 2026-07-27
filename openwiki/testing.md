---
type: Testing Guide
title: Test strategy and verification map
description: Rust, Temporal, tracker, Tauri, Svelte, and operator workflow tests, including opt-in live boundaries and change-oriented commands.
tags: ["testing", "rust", "temporal", "svelte", "tauri"]
---

# Test strategy and verification map

Shea Symphony has broad deterministic coverage around 2606 business rules and growing contract coverage around 2607 foundations. It does not yet have an end-to-end Temporal Main-to-Merge lifecycle test because that lifecycle is not implemented.

## Baseline commands

```bash
cargo test
cargo build
npm --prefix app test
npm --prefix app run check
npm --prefix app run build
```

Use `npm --prefix app run test:build` when validating the static production build contract. Storybook is available through `storybook` and `build-storybook` scripts.

## Test areas

| Area | Main anchors | What it proves |
| --- | --- | --- |
| 2607 Workflow/Activity contracts | unit tests in `src/symphony/**` | durable names, DTOs, queue registrations, no-op and inert outcomes |
| Local SQLite | `src/symphony/local_state/**` tests | migration, identity, health/admin, Describe-backed projection, narrow active reads |
| Coordinator | `src/symphony/coordinator/mod.rs` tests | state classification, optimistic expectations, identity encoding/bounds |
| Temporal smoke | `tests/temporal_noop_smoke.rs` | opt-in worker routing, no-op execution, Query window, result retrieval |
| 2606 lifecycle | `src/main/tests/**`, lane/review module tests | parked human states, claims, routing, recovery, review outcomes |
| Doctor | `src/doctor/tests/**`, command repair tests | evidence gates, topology, diagnostics, bounded repair |
| Parent/subissue | `tests/parent_subissue_topology.rs` | branch/merge-base and parent-owned final review rules |
| App/operator UI | `app/test/*.mjs`, Tauri module tests | Human Todo, handoff prompts, freshness, CLI command construction, events |
| Tracker integrations | `tests/live_github_smoke.rs`, `tests/live_linear_smoke.rs` | opt-in credentialed read-only adapter smoke |
| Skill suite | `tests/skill_suite.rs` | repo-owned skill packaging/contract checks |

## Temporal smoke

Run this ignored integration test only through the single supported repo-owned opt-in path:

```bash
./scripts/temporal-noop-smoke
```

The official `temporal` CLI must be on `PATH` and support `temporal server start-dev`; do not invoke the test directly or start a second server manually. The command sets `SHEA_TEMPORAL_SMOKE=1`, requires the checked-in profile to target `localhost:7233`, and refuses to proceed if a service is already reachable there so it never shares queues with or stops an operator-owned service. Otherwise it starts and cleans up its own non-persistent Temporal dev service and Symphony worker.

A pass proves only the current 2607 no-op runtime spine: normal worker registration, one unique synthetic `IssueWorkflow`, a bounded read-only Query observation, and the terminal no-side-effect `NoopCoreActivity` result with no artifact references. It does **not** exercise a real tracker issue, credentials, tracker transitions, agent Activities, worktrees, SQLite projection, App controls, Coordinator policy, or the Main-to-Merge lifecycle. Do not cite it as evidence of operational orchestration (`scripts/temporal-noop-smoke`, `docs/milestones/2607-hardening/TEMPORAL-NOOP-SMOKE.md`, `tests/temporal_noop_smoke.rs`). The ownership and startup cautions are also summarized in [Operations](operations.md).

## Change-oriented checks

- **Workflow/Activity names or DTOs:** run all `symphony` unit tests and then `./scripts/temporal-noop-smoke` in a clean local environment; assess history/replay compatibility beyond tests.
- **Coordinator identity/activation:** run Coordinator tests and local-state identity/projector tests; confirm static states never receive executable identity.
- **SQLite schema/projection:** run local-state migration, admin, projector, and reader tests; verify no StartResponse-only lifecycle writes.
- **Review/Human Review routing:** run review decision, lane review, main lifecycle, Doctor project-state, and app operator-view tests.
- **Handoff prompts:** update only permitted `.shea/prompts/**` sources and run `app/test/operator-view.test.mjs`.
- **Doctor:** run Doctor unit/command tests and compare suggestions against current merge/review policies; known drift exists for dirty merging PR guidance.
- **App/Tauri CLI bridge:** run Rust tests under `app/src-tauri` plus Node tests; validate both configured external CLI and Cargo fallback assumptions.

## Live-test boundaries

GitHub and Linear smoke tests are opt-in, credential-gated, and read-only. Never broaden them to mutation without an explicit safety contract. Do not read credential files or print tokens. Temporal smoke uses synthetic test-owned identifiers and a bounded query hold so normal product inputs cannot inherit test behavior (`src/symphony/activities.rs`).

## Gaps to keep visible

- no end-to-end real Temporal lifecycle;
- no real `TrackerTransitionActivity` write/readback test;
- no Temporal agent Activity execution;
- no App dashboard backed by SQLite plus selected-detail Temporal Query;
- no demonstrated worker-option enforcement of configured queue concurrency;
- live tracker tests do not prove write or recovery behavior.

These gaps follow directly from the [partial 2607 architecture](architecture/overview.md), not merely missing test effort. Use [Authority and state](architecture/authority-and-state.md) to avoid writing tests that accidentally bless the wrong owner.
