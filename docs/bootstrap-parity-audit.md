# Bootstrap Parity Audit

Status: operator audit snapshot.

Last refreshed: 2026-05-14 from `main` plus live GitHub Project v2 inspection.

This document is the compact completion audit for Jade Symphony. It exists to
keep the project honest while many focused slices are in `Agent Review`: an open
PR is useful evidence, but it is not landed capability until it reaches `Done`
and is present on `main`.

## Reading This Audit

Status meanings:

- `Landed`: present on `main` and covered by local verification or docs.
- `Review Backlog`: implemented in a linked issue/PR waiting outside the main
  implementation lane.
- `Partial`: a first slice exists, but parity or dogfood safety is not complete.
- `Deferred`: intentionally not implemented yet; it remains in the parity
  roadmap.

## Source Contract

Jade Symphony keeps this source order:

1. `docs/bootstrap/references/openai-symphony/SPEC.md`
2. `docs/bootstrap/references/openai-symphony/elixir/README.md`
3. `docs/bootstrap/references/openai-symphony/elixir/WORKFLOW.md`
4. `docs/bootstrap/references/openai-symphony/elixir/lib/`
5. `docs/bootstrap/JADE_SYMPHONY_SPEC.md`
6. `docs/bootstrap/TRACKER_GITHUB_PROJECT_V2.md`
7. `docs/bootstrap/JADE_WORKFLOW.md`
8. `docs/bootstrap/ISSUE_QUALITY_GATE_TEMPLATE.md`

Files under `docs/bootstrap/references/openai-symphony` are reference inputs and
must not be edited by Jade Symphony implementation work.

## Current Mainline Coverage

| Category | Status | Evidence | Remaining gap |
| --- | --- | --- | --- |
| Workflow loading | Landed | `src/workflow.rs`, `README.md`, `docs/dogfood-readiness.md` | Runtime reload with last-known-good config remains deferred. |
| Typed config | Partial | `src/config.rs`, `examples/*.md` | Config is enough for current CLI paths, but richer live worker settings are still evolving. |
| Normalized tracker model | Landed | `src/model.rs`, `src/tracker.rs` | Blocker relationship sources need continued adapter hardening. |
| GitHub Project v2 adapter | Partial | `src/tracker.rs`, `examples/github-project-workflow.md` | Live writes exist behind `--write`; full reconciliation and richer Project field mutation are not fully landed on main. |
| Linear adapter | Partial | `src/tracker.rs`, `examples/linear-fixture-workflow.md` | Live schema smoke coverage is still required before routine use. |
| Issue Quality Gate | Landed | `src/quality_gate.rs`, `docs/bootstrap/ISSUE_QUALITY_GATE_TEMPLATE.md` | Semantic/LLM-assisted checks remain optional and conservative. |
| Issue Forge | Partial | `src/issue_forge.rs`, README command docs | Tracker creation exists; richer field setup and conversational UI remain follow-ups. |
| Workspace lifecycle | Partial | `src/workspace.rs`, `src/handoff.rs`, `src/git_handoff.rs` | Runtime terminal cleanup and remote/SSH parity are not complete. |
| Agent backend abstraction | Partial | `src/agent.rs`, Codex/Claude subprocess workflows | Full Codex app-server and Claude Code protocol parity are deferred. |
| Run loop/orchestrator | Partial | `src/orchestrator.rs`, `src/main.rs`, `src/runtime_state.rs` | Long-running supervision, multi-worker reconciliation, and fully autonomous operation are not complete. |
| Agent Review boundary | Partial | `src/review.rs`, `docs/bootstrap/JADE_WORKFLOW.md` | Bounded review-loop exists; persistent reviewer supervision is still in review/backlog. |
| Merging lane | Partial | `src/merge_lane.rs`, `merge-once` command | Guarded one-shot landing exists; continuous merge-loop and close-after-merge support are not fully landed on main. |
| Observability/status | Partial | `src/status_surface.rs`, `src/event_log.rs`, `src/runtime_state.rs` | Terminal/JSONL surfaces exist; API endpoint work is in review backlog rather than landed. |
| Usage-limit pause/resume | Partial | `src/agent.rs`, `src/runtime_state.rs`, `docs/dogfood-readiness.md` | Vendor-specific quota management and worker-level recovery remain future work. |
| Project doctor | Landed | `src/doctor.rs`, `doctor` / `audit-project` commands | Repair mode is not yet landed on main. |

## Live Project Review Backlog

The following Project items were in `Agent Review` during the 2026-05-14 audit.
They represent pending coverage, not landed capability:

| Issue | Pending capability |
| --- | --- |
| `#66` | Assignee ownership before claim. |
| `#67` | Parallel Review Agent worker selection. |
| `#79` | Project doctor JSON and strict audit mode. |
| `#81` | Merge-once approval and dirty PR routing hardening. |
| `#83` | Bounded merge-loop command. |
| `#85` | Credential-gated live GitHub smoke tests. |
| `#87` | Credential-gated live Linear smoke tests. |
| `#89` | Supervised live dogfood runbook. |
| `#91` | Verification before PR handoff. |
| `#93` | Dirty/no-op PR handoff blocking. |
| `#95` | Terminal workspace cleanup planning. |
| `#97` | Persistent review job ledger evidence. |
| `#99` | Tracker-visible runtime ownership markers. |
| `#101` | Guarded doctor repair for invalid `Human Review`. |
| `#103` | Linked PR discovery from workpad evidence. |
| `#105` | Close linked GitHub issue after guarded merge completion. |
| `#107` | Set Project fields when creating forged issues. |
| `#109` | JSON status snapshot output. |
| `#111` | Local observability API status endpoint. |

When these land, update this audit by moving the relevant rows from review
backlog into mainline coverage.

## Bootstrap Obligations Still Not Complete

These items remain blockers for claiming broad self-running parity:

1. Full Codex app-server protocol parity with session, turn, usage, and
   rate-limit accounting.
2. Full Claude Code protocol parity beyond conservative subprocess execution.
3. Long-running worker supervision with retry, continuation, stall recovery, and
   terminal workspace cleanup wired into reconciliation.
4. Persistent Review Agent supervision that is independent from the main
   implementation lane and can safely pass work to `Human Review`.
5. Credential-gated live GitHub and Linear smoke tests that can run without
   making ordinary local development depend on secrets.
6. Richer Project field mutation and tracker-neutral metadata updates.
7. Runtime workflow reload with last-known-good config behavior.
8. Optional web/API observability after the API endpoint work lands and is
   reconciled with the status snapshot model.
9. Full Liquid-compatible prompt rendering or a deliberately documented
   supported subset.
10. Dynamic tool parity such as the Elixir reference `linear_graphql` tool.

## Current Stop/Continue Guidance

Use this order when continuing the autonomous work loop:

1. Land issues already in `Merging`.
2. Repair `Rework` items produced by failed merge attempts.
3. Pick executable active `In Progress`, then `Todo`, then `Rework` work.
4. If no executable issue exists, create a focused issue from the incomplete
   obligations above using the Issue Quality Gate template.

The main implementation agent may move locally complete work to `Agent Review`.
It must not move work to `Human Review`. Only an independent Review Agent may
set `Human Review`, and only after review evidence is recorded.

## Verification For This Audit

Keep this document aligned by running:

```bash
cargo run -- inspect examples/github-project-workflow.md
cargo run -- doctor examples/github-project-workflow.md
cargo test
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

`inspect` and `doctor` require live GitHub Project access for the non-fixture
workflow. The Cargo verification commands remain local.
