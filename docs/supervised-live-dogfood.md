# Supervised Live Dogfood Runbook

This runbook is for a bounded, human-supervised Jade Symphony dogfood cycle
against GitHub Project v2. Jade Symphony is not a fire-and-forget daemon yet:
operators still approve live write ticks, review handoffs, and merges.

## Role Boundaries

- Main implementation work may stop at `Agent Review`.
- Main implementation work must never set `Human Review`.
- Independent Review Agent work may move a passed review to `Human Review`.
- Humans move accepted work to `Merging`.
- The merge lane consumes only work already in `Merging`.
- Failed, inconclusive, unavailable, or timed-out review must not move to
  `Human Review`.

## Prerequisites

Run from the repository root:

```bash
git status --short --branch
gh auth status
cargo build
cargo test
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

Stop before live dogfood if the worktree is dirty, GitHub auth is unavailable,
or verification is failing for an unexplained reason.

The repo-owned live workflow is:

- `workflows/jade-symphony.md` for implementation, review, merge, smoke,
  inspect, project-state, and Issue Forge commands.

It defaults durable artifacts to `~/.jade-symphony/artifacts` when
`JADE_SYMPHONY_ARTIFACT_ROOT` is unset. Set that variable to migrate worktrees,
logs, runtime state, review prompts, and review ledgers to another local root.
If a command points at `/tmp/*.md` or `/private/tmp/*.md`, promote the reusable
workflow or prompt into `workflows/`, `examples/`, or `docs/` before treating it
as canonical. Normal dogfood workflow config belongs in `workflows/`.

## Inspect The Project

Use the live workflow as the source of tracker state:

```bash
cargo run -- inspect workflows/jade-symphony.md
cargo run -- doctor workflows/jade-symphony.md
```

Review the output before mutating anything. In particular, check for:

- `Rework` items produced by failed merge attempts;
- issues in `Merging`;
- other issues in `Rework`;
- active `In Progress` work;
- Agent Review issues missing PR evidence;
- dirty Merging PRs;
- integration gaps.

## Controlled Smoke

Before the first write tick, run the controlled smoke preflight:

```bash
cargo run -- dogfood-smoke workflows/jade-symphony.md --dry-run
```

Proceed only when there is exactly one executable controlled smoke candidate,
the tracker is not fixture-backed, and the report shows no blocking integration
gaps.

## One Implementation Tick

Preview the same bounded implementation tick without tracker mutation:

```bash
cargo run -- run-loop workflows/jade-symphony.md --max-iterations 1 --dry-run
```

Use one bounded write tick:

```bash
cargo run -- run-loop workflows/jade-symphony.md --max-iterations 1 --write
```

The operator launcher wraps the same workflow with local preflight checks:

```bash
scripts/jade-dogfood --dry-run
scripts/jade-dogfood --write --confirm-write --max-iterations 1
```

Expected outcome for successful main-agent work:

- the issue is claimed or resumed safely;
- the issue workspace and branch are prepared;
- the configured backend runs in that workspace;
- runtime state and event logs are written;
- a PR is created or reused;
- the workpad records durable handoff evidence;
- the issue moves to `Agent Review`, not `Human Review`.

If the run reports usage limits, retry backoff, missing PR evidence, failed
handoff, stale runtime state, or missing human input, stop and resolve that
specific blocker before running another write tick.

## Agent Review

For a bounded Review Agent pass, run:

```bash
export JADE_GEMINI_COMMAND="$(command -v gemini)"
cargo run -- review-loop workflows/jade-symphony.md --max-iterations 1 --write
```

Expected outcomes:

- each write-mode Review Agent records a `Review Agent` Project field claim
  before launching the backend;
- passed independent review may move the issue to `Human Review`;
- confirmed findings move the issue to `Rework`;
- failed, inconclusive, timed-out, or unavailable review stays out of
  `Human Review` and records evidence.

If the review workflow uses `review.gemini_command: $JADE_GEMINI_COMMAND`,
set that environment variable to an absolute Gemini CLI path before starting
the supervised review loop. This avoids worker sessions with a narrower `PATH`
recording a backend-unavailable failure for an otherwise installed Gemini CLI.

Do not use review commands to bypass human acceptance.

## Human Review And Merging

After a human accepts work and moves the issue to `Merging`, use the guarded
merge lane:

```bash
cargo run -- merge-once workflows/jade-symphony.md --dry-run
cargo run -- merge-once workflows/jade-symphony.md --write
```

The merge lane should:

- inspect only `Merging` issues;
- require exactly one linked PR;
- check PR state, base branch, checks, review/approval signal, and mergeability;
- merge clean approved work;
- route dirty or failing work to `Rework` with workpad evidence;
- retry transient missing or `UNKNOWN` mergeability instead of treating it as a
  human decision;
- include a `Required Human Input` question whenever a blocker really needs a
  human answer;
- never set `Human Review`.

After each merge pass, run the Rust verification suite again on `main`.

```bash
cargo test
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

Then refresh the tracker and project invariants:

```bash
cargo run -- inspect workflows/jade-symphony.md
cargo run -- doctor workflows/jade-symphony.md
```

## Recovery

When something goes wrong:

- use `cargo run -- inspect workflows/jade-symphony.md` to refresh
  tracker state;
- use `cargo run -- doctor workflows/jade-symphony.md` to find project
  invariant violations;
- inspect runtime state under the configured logs root;
- continue existing issue branches/PRs for rework rather than creating duplicate
  branches;
- keep one issue, one branch, one PR.

If a branch is dirty because `main` advanced, repair the existing PR branch,
record workpad evidence, and rerun verification before merging.

## Stop Conditions

Stop the live dogfood loop when:

- there are no executable `Todo`, `Rework`, active `In Progress`, or `Merging`
  issues;
- credentials, external services, or sample data are missing;
- a human decision is required;
- destructive action would be required;
- verification fails and cannot be locally diagnosed;
- usage limits require backoff.

Stopping at these boundaries is part of the safety model, not a failure.
When the queue is empty and the bootstrap audit still shows incomplete parity
obligations, create one focused Project issue from the audit rather than
starting untracked implementation work.
