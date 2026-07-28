---
type: Testing Guide
title: Test strategy and verification map
description: Rust, Temporal, tracker, Tauri, Svelte, and operator workflow tests, including opt-in live boundaries and change-oriented commands.
tags: ["testing", "rust", "temporal", "svelte", "tauri"]
---

# Test strategy and verification map

Shea Symphony has broad deterministic coverage around 2606 business rules and growing contract coverage around 2607 foundations. The [protected 2606 runtime](architecture/2606-bootstrap-runtime.md) supplies behavior, tests, and operational evidence as an acceptance oracle, not an orchestration/runtime implementation that 2607 tests should keep alive. Focused test cases and bounded protocol-independent Rust components may be reused after ownership review when they fit the new typed boundary and retain focused coverage. The repository does not yet have an end-to-end Temporal Main-to-Merge lifecycle test because that lifecycle is not implemented. **Tracking:** lifecycle implementation belongs to T2607-05/06; no Issues are promoted for either package.

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
- **Doctor:** run Doctor unit/command tests and compare suggestions against current merge/review policies. #390 is Done and established the newer merge-agent policy; any retained 2606 dirty-merge repair is an unowned gap or removable under T2607-08, for which no Issue is promoted.
- **App/Tauri bootstrap bridge:** run Rust tests under `app/src-tauri` plus Node tests; validate separate engine/target roots and explicit protected-2606 CLI selection against canonical main. Verify that current-main Cargo fallback is not treated as a complete product CLI.
- **2607 replacement acceptance:** translate protected-2606 behavior, safety, failure, and recovery evidence into tests of new Workflow, Activity, Coordinator, Tauri, SQLite, and operator-action contracts. A test that passes by invoking legacy CLI/lane implementation does not prove migration.

## Live-test boundaries

GitHub and Linear smoke tests are opt-in, credential-gated, and read-only. Never broaden them to mutation without an explicit safety contract. Do not read credential files or print tokens. Temporal smoke uses synthetic test-owned identifiers and a bounded query hold so normal product inputs cannot inherit test behavior (`src/symphony/activities.rs`).

## Gaps to keep visible

- no end-to-end real Temporal lifecycle — **Tracking:** T2607-05/06; no Issues promoted;
- no real `TrackerTransitionActivity` write/readback test — **Tracking:** #494 is Done only for the transition DTO/idempotency contract; remaining T2607-04 mutation/evidence/reconcile work has no promoted Issue;
- no Temporal agent Activity execution — **Tracking:** T2607-05; no Issue promoted;
- no App dashboard backed by SQLite plus selected-detail Temporal Query — **Tracking:** T2607-07; no Issue promoted, and #505 is only a bounded backend Backlog seed excluding Svelte UX;
- no demonstrated worker-option enforcement of configured queue concurrency — **Tracking:** unowned T2607-01 residual that must be settled before T2607-05/06 rely on it;
- live tracker tests do not prove write or recovery behavior — **Tracking:** remaining transition behavior belongs to T2607-04 with no promoted Issue; wider recovery acceptance belongs to the relevant T2607-05/06/07/08 packages, also with no promoted Issues.

These gaps follow directly from the [partial 2607 architecture](architecture/overview.md), not merely missing test effort. Use [Authority and state](architecture/authority-and-state.md) to avoid writing tests that accidentally bless the wrong owner.
