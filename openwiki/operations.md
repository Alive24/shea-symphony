---
type: Runbook
title: Build, run, and recovery guidance
description: Practical current-main and protected-2606 setup, execution, readiness, diagnostics, and recovery guidance for Shea Symphony.
tags: ["operations", "runbook", "temporal", "build", "recovery"]
---

# Build, run, and recovery guidance

## Know which runtime you are operating

Current `main` builds a local Temporal worker host (`src/main.rs`). The protected 2606 MVP contains the proven product CLI/autoloop behavior described by much of `docs/cli-command-reference.md` and `docs/operator-dogfood.md`. The desktop app also still expects legacy CLI read/autoloop commands unless a profile-specific runtime is configured.

Do not assume `cargo run -- doctor ...` or `cargo run -- autopilot ...` on current `main` invokes the old CLI. This is the most important operational drift in the repository. See [Architecture](architecture/overview.md).

## Current-main Temporal worker

Prerequisites evidenced by manifests/config:

- Rust toolchain compatible with edition 2021;
- local Temporal service reachable at the workflow-configured address, default `localhost:7233`;
- checked-in workflow `.shea/workflows/shea-symphony.md`, or `SHEA_WORKFLOW_PATH` pointing to another valid profile.

```bash
cargo build
cargo run
```

The process loads and validates the workflow, connects to Temporal, registers core/agent/local workers, and blocks while workers run. The current product path is only a no-op skeleton; successful startup does not prove tracker, agent, projection, or lifecycle execution.

For the repo-owned integration smoke, use the single supported opt-in path:

```bash
./scripts/temporal-noop-smoke
```

The official `temporal` CLI must be on `PATH` with `temporal server start-dev` support. The command sets the opt-in environment itself, starts and later reaps its own non-persistent dev service and worker, and refuses to run when any service is already reachable at `localhost:7233`; it will not share, inspect, or stop an operator-owned service. A pass proves only worker registration plus one synthetic no-op Workflow start, read-only Query window, and terminal `NoopCoreActivity` result. It does not use or prove tracker transitions, agent execution, worktrees, SQLite projection, artifacts, App controls, or a real lifecycle (`docs/milestones/2607-hardening/TEMPORAL-NOOP-SMOKE.md`, `tests/temporal_noop_smoke.rs`). See [Testing](testing.md) for the verification boundary.

Use `app/src-tauri/src/temporal_health.rs` as the operator-safe readiness contract: `ready`, `unavailable`, `timedOut`, or `invalidConfig`, with a five-second bound and captured workspace identity.

## Desktop app

```bash
npm --prefix app install
npm --prefix app run dev
npm --prefix app run tauri -- dev
```

Useful checks:

```bash
npm --prefix app test
npm --prefix app run check
npm --prefix app run build
```

The Tauri app calls legacy CLI JSON/read surfaces for most dashboard data and controls Autoloop through a child process. On current `main`, configure a valid external/profile 2606 CLI if using those product operations; Cargo fallback now targets the Temporal-only root binary.

## Protected 2606 operations

For supervised lane operation, use the protected 2606 runtime and follow `docs/operator-dogfood.md` rather than reconstructing commands here. Its safety pattern is:

1. run read-only status/Doctor/project inspection;
2. preview plans or dry-runs;
3. use bounded foreground write iterations;
4. preserve tracker, PR, workpad, claim, session, worktree, and artifact evidence;
5. reconcile uncertain writes through readback rather than blind retry;
6. audit cleanup only after work is accounted for.

[Operator workflows](workflows/operator-workflows.md) summarizes lane-specific recovery and confirmation gates.

## Configuration and integrations

The checked-in workflow config connects:

- GitHub Project v2 tracker and its state map;
- local Temporal namespace/address and three task queues;
- lane prompt and workpad template files;
- Codex app-server for Main work and explicit bounded merge diagnosis or repair; clean `merge once` and `merge loop` landing is direct deterministic CLI behavior and does not require an agent session;
- `agy` CLI for independent Review;
- workspace, artifact, polling, verification, and observability settings.

Treat machine-specific command paths in the checked-in profile as local configuration, not portable defaults. Do not document or expose credentials. GitHub and Linear live tests are explicitly credential-gated and read-only.

## Failure triage

| Symptom | First check | Boundary |
| --- | --- | --- |
| Worker cannot start | workflow parse/config, Temporal service readiness | current 2607 host |
| No real issue movement | expected: `IssueWorkflow` is no-op and transition/agent Activities are placeholders | implementation status |
| App dashboard command failures | selected workspace and configured legacy CLI path | 2606 app integration |
| Stuck issue or missing evidence | Doctor plus tracker/PR/runtime/worktree reads | operator recovery |
| SQLite row conflicts | current Temporal Describe evidence; do not delete/reserve blindly | local projection |
| Uncertain tracker write | targeted readback and recovery marker/evidence | tracker authority |

## Operational cautions

- Do not infer live issue/project status from milestone or implementation docs.
- Do not use SQLite rows as permission to start, resume, or transition work.
- Do not clear claims, sessions, or worktrees merely to remove a warning.
- Temporal Workflow/Activity names and DTO serialization are durable compatibility surfaces.
- The configured 3/3/8 queue concurrency is visible in metadata, but inspected worker builders do not clearly enforce it.

## Related pages

- [Authority and state](architecture/authority-and-state.md) for reconciliation rules.
- [Testing](testing.md) for smoke-test prerequisites and targeted verification.
- [Source map](source-map.md) for canonical docs and implementation entrypoints.
