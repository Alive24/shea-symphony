# Bootstrap Parity Audit

Status: operator audit snapshot.

Last refreshed: 2026-05-15 from `origin/main`
(`f69bea6e8c57b512310097078de0efa535a9cd8f`) plus live GitHub Project v2
inspection.

This document is the compact completion audit for Shea Symphony. It exists to
keep the project honest as focused slices move through review and merge: an open
PR is useful evidence, but it is not landed capability until it reaches `Done`
and is present on `main`.

## Reading This Audit

Status meanings:

- `Landed`: present on `main` and covered by local verification or docs.
- `Partial`: a first slice exists, but parity or dogfood safety is not complete.
- `Deferred`: intentionally not implemented yet; it remains in the parity
  roadmap.

## Source Contract

Shea Symphony keeps this source order:

1. `docs/bootstrap/references/openai-symphony/SPEC.md`
2. `docs/bootstrap/references/openai-symphony/elixir/README.md`
3. `docs/bootstrap/references/openai-symphony/elixir/WORKFLOW.md`
4. `docs/bootstrap/references/openai-symphony/elixir/lib/`
5. `docs/bootstrap/SHEA_SYMPHONY_SPEC.md`
6. `docs/bootstrap/TRACKER_GITHUB_PROJECT_V2.md`
7. `docs/bootstrap/SHEA_WORKFLOW.md`
8. `docs/bootstrap/ISSUE_QUALITY_GATE_TEMPLATE.md`

Files under `docs/bootstrap/references/openai-symphony` are reference inputs and
must not be edited by Shea Symphony implementation work.

## Current Mainline Coverage

| Category | Status | Evidence | Remaining gap |
| --- | --- | --- | --- |
| Workflow loading | Partial | `src/workflow.rs`, `README.md`, `docs/dogfood-readiness.md`, `workflows/shea-symphony.md` | A first-slice reload store exists; long-running runtime reload wiring remains deferred. |
| Typed config | Partial | `src/config.rs`, `examples/*.md` | Config is enough for current CLI paths, but richer live worker settings are still evolving. |
| Normalized tracker model | Landed | `src/model.rs`, `src/tracker.rs` | Blocker relationship sources need continued adapter hardening. |
| GitHub Project v2 adapter | Partial | `src/tracker.rs`, `workflows/shea-symphony.md`, `project state` command | Live reads/writes exist behind `gh` and explicit `--write`; full reconciliation and richer Project field mutation are not complete. |
| Linear adapter | Partial | `src/tracker.rs`, `examples/linear-fixture-workflow.md` | Live schema smoke coverage is still required before routine use. |
| Issue Quality Gate | Landed | `src/quality_gate.rs`, `docs/bootstrap/ISSUE_QUALITY_GATE_TEMPLATE.md` | Semantic/LLM-assisted checks remain optional and conservative. |
| Issue Forge | Partial | `src/issue_forge.rs`, README command docs | Tracker creation exists; richer field setup and conversational UI remain follow-ups. |
| Workspace lifecycle | Partial | `src/workspace.rs`, `src/handoff.rs`, `src/git_handoff.rs`, `clean plan` command | Terminal cleanup planning exists; automatic runtime cleanup and remote/SSH parity are not complete. |
| Agent backend abstraction | Partial | `src/agent.rs`, Codex/Claude subprocess workflows | Full Codex app-server and Claude Code protocol parity are deferred. |
| Run loop/orchestrator | Partial | `src/orchestrator.rs`, `src/main.rs`, `src/runtime_state.rs`, `autopilot plan`, `autopilot loop` | Bounded foreground all-lane supervision exists; unbounded daemon/background operation, richer reconciliation, and fully autonomous operation are not complete. |
| Agent Review boundary | Partial | `src/review.rs`, `review loop`, review job ledger docs | Bounded `review loop` and durable review evidence exist; persistent background reviewer supervision is still incomplete. |
| Merging lane | Partial | `src/merge_lane.rs`, `merge once`, `merge loop` command | Guarded one-shot and bounded pool landing exist; unbounded continuous merge polling and richer reconciliation are not complete. |
| Observability/status | Partial | `src/status_surface.rs`, `src/event_log.rs`, `src/runtime_state.rs`, `status serve` command | Terminal, JSONL, JSON snapshot, local one-shot API, and tracker mutation audit surfaces exist; persistent/remote web service mode remains incomplete. |
| Usage-limit pause/resume | Partial | `src/agent.rs`, `src/runtime_state.rs`, `docs/dogfood-readiness.md` | Vendor-specific quota management and worker-level recovery remain future work. |
| Project doctor | Partial | `src/doctor.rs`, `doctor` / `audit-project`, `doctor-repair-human-review` commands | Strict/JSON audit and one targeted repair exist; broader repair mode remains a follow-up. |

## Live Project Queue Snapshot

Live Project #9 inspection on 2026-05-15 showed the prior 2026-05-14 review and
merge backlog landed as `Done` on `main`, including the old `#66` through
`#191` capability slices. The only non-terminal item created during this audit
refresh was `#199`, the documentation reconciliation issue that produced this
update.

Do not recreate the old backlog from this document. When the queue is empty,
create a focused issue from the incomplete obligations below, add it to Project
#9, and claim it through the appropriate lane field before editing.

## Bootstrap Obligations Still Not Complete

These items remain blockers for claiming broad self-running parity:

1. Full Codex app-server protocol parity with session, turn, usage, and
   rate-limit accounting.
2. Full Claude Code protocol parity beyond conservative subprocess execution.
3. Persistent or unbounded worker supervision with retry, continuation, stall
   recovery, and terminal workspace cleanup wired into reconciliation. Bounded
   foreground Autoloop is not a daemon and does not close this obligation
   by itself.
4. Persistent Review Agent supervision that is independent from the main
   implementation lane and can safely pass work to `Human Review`.
5. Mutation-capable credential-gated live GitHub and Linear smoke tests that can
   run without making ordinary local development depend on secrets.
6. Richer Project field mutation and tracker-neutral metadata updates.
7. Runtime workflow reload wiring that uses the last-known-good config behavior
   during long-running loops.
8. Persistent web/API observability reconciled with the status snapshot model.
9. Full Liquid-compatible prompt rendering beyond the documented supported
   subset, if parity requires it.
10. Dynamic tool parity such as the Elixir reference `linear_graphql` tool.

## Current Stop/Continue Guidance

Use this order when continuing the autonomous work loop:

1. Repair `Rework` items produced by failed merge attempts.
2. Land issues already in `Merging`.
3. Pick executable active `In Progress`, then `Todo`, then other `Rework` work.
4. If no executable issue exists, create a focused issue from the incomplete
   obligations above using the Issue Quality Gate template.

Use `Merging Agent` as the claim field for `Rework` and `Merging` lanes. Use
`Main Agent` as the claim field for `Todo` and `In Progress` implementation
lanes. These Project fields are advisory claim-lock flags for parallel local
sessions; re-read the item before and after writes.

The main implementation agent may move locally complete work to `Agent Review`.
It must not move work to `Human Review`. Only an independent Review Agent may
set `Human Review`, and only after review evidence is recorded.

## Verification For This Audit

Keep this document aligned by running:

```bash
cargo run -- project inspect workflows/shea-symphony.md '#<issue>'
cargo run -- project state workflows/shea-symphony.md
cargo run -- doctor workflows/shea-symphony.md
cargo test
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

`inspect` and `doctor` require live GitHub Project access for the non-fixture
workflow. The Cargo verification commands remain local.
