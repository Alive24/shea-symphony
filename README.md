# Jade Symphony

Jade Symphony is a supervised orchestration harness for local coding agents.
It helps an operator turn tracked engineering work into isolated agent
workspaces, pull requests, independent review passes, and guarded merge
decisions without losing the human control points that make the process safe.

The project is an OpenAI Symphony-style Rust implementation with Jade Symphony-specific
extensions for tracker-driven team workflows. It is inspired by the official
OpenAI Symphony specification and reference implementation, and extends that
lineage with GitHub Project v2 / Linear state machines, issue quality gates,
agent role lanes, durable evidence, and supervised dogfood workflows.

Current maturity: **supervised dogfood, not unattended production
orchestration**. Jade Symphony can run bounded implementation, review, and merge
ticks against a live tracker, but it is still intentionally operator-led.

## Why It Exists

Local coding agents are powerful, but team work needs more than a prompt and a
terminal. Real engineering flow needs scoped issues, ownership, branch hygiene,
review evidence, merge gates, restart behavior, and a clear answer to "what is
happening right now?"

Jade Symphony is built around that gap. It treats the tracker as the source of
truth, then gives agents a narrow lane:

- Main Agent work starts from an executable issue contract and stops at
  `Agent Review`.
- Review Agent work independently inspects the PR and records pass or rework
  evidence before anything can reach `Human Review`.
- Merge Agent work handles the guarded `Merging` lane, including dirty PRs,
  conflicts, failed checks, and durable diagnostics.

The goal is not to remove the operator. The goal is to make supervised agent
work repeatable enough that a human can safely keep several pieces of work
moving without turning the repo, tracker, or local machine into mystery state.

## Relationship To OpenAI Symphony

Jade Symphony does not compete with OpenAI Symphony. It uses the official
Symphony specification and reference implementation as the baseline lineage for
workflow loading, tracker normalization, agent execution, runtime state,
workspace lifecycle, structured logs, and operator status surfaces.

Jade Symphony adds a pragmatic layer for local, tracker-backed engineering
teams:

- GitHub Project v2 and Linear tracker state machines;
- issue contracts and an Issue Quality Gate before dispatch;
- separate Main Agent, Review Agent, and Merge Agent lanes;
- isolated per-issue worktrees, branches, pull requests, and workpad evidence;
- logical actor audit records without requiring separate GitHub accounts;
- local backend orchestration for Codex, Claude Code, and Gemini review;
- supervised dogfood commands for bounded live runs.

The source references and parity expectations live in
[`docs/bootstrap/`](docs/bootstrap/), including the pinned official reference
material under
[`docs/bootstrap/references/openai-symphony`](docs/bootstrap/references/openai-symphony/).

## What You Can Do Today

Jade Symphony can already support a supervised local dogfood loop:

- read live GitHub Project v2 or Linear-backed tracker state through normalized
  issue records;
- validate workflow files and inspect executable queue state;
- gate issues for required fields, dependency semantics, referenced paths, and
  verification commands;
- create or reuse isolated issue worktrees and branches;
- run bounded `main loop`, `review loop`, and `merge loop` ticks with explicit
  write-mode confirmation;
- write tracker-visible workpad evidence and local JSONL audit records;
- create or reuse PR handoffs for completed Main Agent work;
- route review failures, merge conflicts, dirty PRs, and runtime failures to
  visible follow-up states instead of silently advancing them;
- show operator status, latest-lane summaries, and doctor/audit diagnostics.

It is still not a hands-off daemon. Long-running worker supervision, full Codex
app-server transport, richer multi-worker resume reconciliation, automatic
terminal workspace cleanup, and hosted/remote observability remain active
follow-up work.

For the current operator workflow, start with
[`docs/operator-dogfood.md`](docs/operator-dogfood.md). For the command surface,
see [`docs/cli-command-reference.md`](docs/cli-command-reference.md). The live
self-dogfood workflow is
[`workflows/jade-symphony.md`](workflows/jade-symphony.md). For the detailed
capability inventory and parity status that used to dominate this README, see
[`docs/dogfood-readiness.md`](docs/dogfood-readiness.md) and
[`docs/bootstrap-parity-audit.md`](docs/bootstrap-parity-audit.md).

## How The Loop Works

Jade Symphony expects work to move through tracker state, not private terminal
memory.

1. An issue is drafted or repaired into an executable contract.
2. The Issue Quality Gate decides whether it is safe to dispatch.
3. The Main Agent claims the tracker lane, works in an isolated branch/worktree,
   verifies locally, opens or reuses one PR, records evidence, and stops at
   `Agent Review`.
4. The Review Agent runs independently, writes a review ledger, and moves the
   issue to `Human Review` only on passing evidence. Confirmed findings go to
   `Rework`.
5. Human approval moves the issue into `Merging`.
6. The Merge Agent performs guarded merge handling and either lands the PR,
   records a blocker, or routes the issue back to the correct state.

This separation is deliberate. A Main Agent cannot approve its own work, and a
merge repair does not erase the need for fresh review when the repair is
semantic or uncertain.

## Operator Quickstart

Build and run the safe local checks:

```bash
cargo build
cargo test
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

Inspect the canonical dogfood workflow:

```bash
cargo run -- validate workflows/jade-symphony.md
cargo run -- project state workflows/jade-symphony.md
cargo run -- project inspect workflows/jade-symphony.md '#284'
cargo run -- doctor workflows/jade-symphony.md
```

Run a bounded supervised preview:

```bash
scripts/jade-dogfood --dry-run
cargo run -- main loop workflows/jade-symphony.md --max-iterations 1 --dry-run
cargo run -- review loop workflows/jade-symphony.md --max-iterations 1 --dry-run
cargo run -- merge loop workflows/jade-symphony.md --max-iterations 1 --dry-run
```

Live writes are explicit and should stay bounded:

```bash
scripts/jade-dogfood --write --confirm-write --max-iterations 1
```

Use the CLI reference for detailed command behavior, write boundaries, and lane
authority rules:
[`docs/cli-command-reference.md`](docs/cli-command-reference.md).

## Issue Forge

Issue Forge turns complete issue bodies into Project-aware tracker mutations.
Conversation, reflection, and draft repair now live in Codex skills; the Jade Symphony CLI stays deterministic and scriptable.

Doctor triage for `Need Human Input` and operator-selected stuck states lives in
the repo-owned skill `.codex/skills/jade-symphony-doctor/SKILL.md`, with the
operator spec in `docs/operator-doctor.md`. It preserves evidence and recommends
confirmation-gated repairs instead of mutating Project state by default.

Typical dry-run entrypoints:

```bash
cargo run -- forge validate --status Backlog --title "Backlog seed" --body-file examples/fixtures/repaired-issue.md
cargo run -- forge validate --status Todo --title "Executable issue" --body-file examples/fixtures/repaired-issue.md
cargo run -- forge create --status Backlog --title "Backlog: follow-up" --body-file examples/fixtures/repaired-issue.md --dry-run
```

Tracker creation requires explicit write flags, an assignee, and project
selection. See the command reference before using live creation:
[`docs/cli-command-reference.md`](docs/cli-command-reference.md).

## Project Layout

- [`workflows/jade-symphony.md`](workflows/jade-symphony.md): canonical
  self-dogfood workflow for Project #9.
- [`workflows/prompts/`](workflows/prompts/): lane-specific Main, Review, and
  Merge Agent prompt contracts.
- [`docs/operator-dogfood.md`](docs/operator-dogfood.md): supervised operator
  launcher and live-run guidance.
- [`docs/cli-command-reference.md`](docs/cli-command-reference.md): command
  behavior, write boundaries, and examples.
- [`skills/jade-symphony/`](skills/jade-symphony/): repo-owned, dated
  installable Jade Symphony skills for Codex and Gemini operator sessions.
- [`docs/dogfood-readiness.md`](docs/dogfood-readiness.md): current readiness
  and known gaps.
- [`docs/bootstrap-parity-audit.md`](docs/bootstrap-parity-audit.md): detailed
  capability inventory and parity status.
- [`docs/artifact-storage-policy.md`](docs/artifact-storage-policy.md):
  durable, recoverable, and disposable artifact policy.
- [`docs/bootstrap/`](docs/bootstrap/): Jade Symphony extension spec, workflow notes,
  parity references, and official Symphony source index.
- [`examples/`](examples/): fixture workflows and safe local examples.

## Design Boundaries

Jade Symphony is orchestration infrastructure. It should not contain downstream
application business logic. Domain-specific work belongs in tracked issues and
per-issue workspaces.

The tracker remains the operating source of truth. Local runtime files are used
for recovery and audit, but live status must be refreshed from the tracker
before deciding what to claim, review, or merge.

Write-mode commands are intentionally explicit. Jade Symphony should record
evidence before state transitions, preserve role boundaries, and prefer a
visible blocked state over an unsafe silent advance.

## Development

The main verification commands are:

```bash
cargo test
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

Useful read-only commands:

```bash
cargo run -- validate examples/dry-run-workflow.md
cargo run -- project inspect examples/dry-run-workflow.md '#1'
cargo run -- plan examples/dry-run-workflow.md
cargo run -- status show examples/dry-run-workflow.md --json
cargo run -- clean plan workflows/jade-symphony.md
```

The implementation is grounded in `docs/bootstrap/` and the pinned official
reference under `docs/bootstrap/references/openai-symphony/`.

Do not edit files under `docs/bootstrap/references/openai-symphony/`.
