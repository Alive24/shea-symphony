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
  inspect, project state, and Issue Forge commands.

It defaults durable artifacts to `~/.jade-symphony/artifacts` when
`JADE_SYMPHONY_ARTIFACT_ROOT` is unset. Set that variable to migrate worktrees,
logs, runtime state, review prompts, and review ledgers to another local root.
If a command points at `/tmp/*.md` or `/private/tmp/*.md`, promote the reusable
workflow or prompt into `workflows/`, `examples/`, or `docs/` before treating it
as canonical. Normal dogfood workflow config belongs in `workflows/`.

## Inspect The Project

Use the live workflow as the source of tracker state:

```bash
cargo run -- project inspect workflows/jade-symphony.md '#<issue>'
cargo run -- doctor
cargo run -- doctor --interactive
```

Review the output before mutating anything. In particular, check for:

- issues in `Merging`;
- `Rework` items that need Main/Review-directed repair;
- historical merge-lane recovery items only when an operator explicitly chose
  that path;
- active `In Progress` work;
- Agent Review issues missing PR evidence;
- dirty Merging PRs;
- integration gaps.

## Normal Preflight

Before the first write tick, run the same preflight surfaces operators use for
normal work:

```bash
cargo run -- project state workflows/jade-symphony.md
cargo run -- doctor workflows/jade-symphony.md
cargo run -- main loop workflows/jade-symphony.md --max-iterations 1 --dry-run
```

Proceed only when tracker access is trusted, the selected issue is claimable,
and the dry-run shows no blocking integration gaps. Use `debug` when a compact
human-readable readiness report is useful.

## One Implementation Tick

Preview the same bounded implementation tick without tracker mutation:

```bash
cargo run -- main loop workflows/jade-symphony.md --max-iterations 1 --dry-run
```

Use one bounded write tick:

```bash
cargo run -- main loop workflows/jade-symphony.md --max-iterations 1 --write
```

That write tick requires a real main-agent backend. The canonical workflow uses
`main_lane.backend: tmux`, so a successful tick starts an attachable local session,
prints the `tmux attach-session` command, records the prompt artifact, session
registry entry, and log path, and keeps the issue active. A running tmux session
alone is not completion evidence and must not move the issue to `Agent Review`.
Run another bounded `main loop --write` tick after the Main Agent session
finishes. The loop first reconciles the recorded runtime/session registry entry;
only a terminal completed session proceeds to verification, PR publication,
linked-PR readback, PR readiness, and the final `Agent Review` state change.
Active, waiting, unknown, or missing-registry sessions are kept out of duplicate
launch and out of `Agent Review`.
Codex tmux startup captures the pane before sending the issue prompt. By
default, if a Jade Symphony-created issue worktree shows the Codex workspace trust
prompt, Jade Symphony sends two `C-m` submissions and waits until the pane reaches a
ready Codex viewport. Set `JADE_SYMPHONY_TMUX_AUTO_TRUST=0` to disable this
auto-trust behavior. If the prompt cannot be cleared, the write tick stops with
the tmux attach command and log path preserved for manual inspection.
Use `cargo run -- status workflows/jade-symphony.md` for compact session
classification and attach/log evidence, `cargo run -- doctor
workflows/jade-symphony.md` for stale or mismatched runtime/session findings,
and `cargo run -- clean audit workflows/jade-symphony.md` to classify session
artifacts before cleanup.

The operator launcher wraps the same workflow with local preflight checks:

```bash
scripts/jade-dogfood --dry-run
scripts/jade-dogfood --write --confirm-write --max-iterations 1
```

Expected outcome for successful main-agent work:

- the issue is claimed or resumed safely;
- the issue workspace and branch are prepared;
- `workspace show` can discover the issue workspace from registry, workpad, PR,
  and local git worktree evidence;
- the configured backend runs in that workspace;
- runtime state and event logs are written;
- a PR is created or reused;
- the PR relationship is verified through the Project/issue linked-PR read
  surface;
- the linked PR is ready, not draft;
- the workpad records durable handoff evidence;
- the issue moves to `Agent Review`, not `Human Review`.

If the run reports usage limits, retry backoff, missing or unverified PR
relationship evidence, draft PR handoff, failed handoff, stale runtime state,
or missing human input, stop and resolve that specific blocker before running
another write tick.

Before manual Review or Merge recovery, check the issue workspace first:

```bash
cargo run -- workspace show workflows/jade-symphony.md '#253'
```

If multiple strong candidates appear, choose the correct Main PR worktree and
record it explicitly:

```bash
cargo run -- workspace adopt workflows/jade-symphony.md '#253' /path/to/worktree --write
```

If no suitable candidate exists, prepare the Review/Merge inspection workspace
through Jade Symphony instead of checking out the PR in the canonical checkout:

```bash
cargo run -- workspace ensure workflows/jade-symphony.md '#253' --dry-run
cargo run -- workspace ensure workflows/jade-symphony.md '#253' --pr 254 --write
```

`workspace ensure --write` requires the canonical checkout to be clean latest
`main`, creates only below the configured workspace root, and records
`### Workspace Evidence` in the issue workpad for later `workspace show`,
Review, Merge, and Doctor flows.

## Agent Review

For a bounded Review Agent pass, run:

```bash
export JADE_GEMINI_COMMAND="$(command -v gemini)"
cargo run -- review loop workflows/jade-symphony.md --max-iterations 1 --write
```

For a manual/operator-supplied review, use the same CLI authority boundary
instead of editing the Project board directly:

```bash
cargo run -- review claim workflows/jade-symphony.md '#226' --worker "Manual Gemini Review" --write
cargo run -- review pass workflows/jade-symphony.md '#226' --evidence-file /tmp/review-evidence.md --write
cargo run -- review reject workflows/jade-symphony.md '#226' --evidence-file /tmp/review-evidence.md --target-state rework --write
```

Expected outcomes:

- each write-mode Review Agent records a `Review Agent` Project field claim
  before launching the headless review job;
- Gemini-backed `review loop` invokes the configured Gemini command headlessly
  with `--prompt`, `--output-format json`, workflow-configured model, and
  workflow-configured interim allowed tools, writes the prompt through stdin,
  and records stdout, stderr, exit status, Gemini session id when present, a
  review output artifact, durable job ledger, and append-only Agent Review
  timeline comment;
- manual review claims use `review claim`, and terminal manual review routing
  validates the exact evidence claim before preserving the `Review Agent` field
  as a terminal structured audit pointer;
- worker display labels such as `Manual Gemini Review` are stored through
  CLI-owned quoting/escaping; avoid raw Project edits for normal claim repair;
- passed independent review may move the issue to `Human Review`;
- confirmed findings move the issue to `Rework`;
- completed but inconclusive automatic review moves to `Rework` with a missing
  evidence diagnostic;
- failed, timed-out, unavailable, unparsed, or infrastructure-blocked review
  stays out of `Human Review` with durable evidence.

If the review workflow uses `review_lane.gemini_command: $JADE_GEMINI_COMMAND`,
set that environment variable to an absolute Gemini CLI path before starting
the review loop. This avoids worker processes with a narrower `PATH` recording
a backend-unavailable failure for an otherwise installed Gemini CLI.

Supervised tmux Review remains available for operator-controlled review through
`review claim` followed by `session start --lane review --run <RUN_ID>`, but it
is no longer the default automatic `review loop` backend.

Do not use review commands to bypass human acceptance.

## Human Review And Merging

Human Review requires durable pass evidence from the independent Review Agent.
For GitHub Project #9, do not assume the `Review Agent` claim field is enough:
doctor expects the canonical Agent Review timeline comment pass marker in the
issue comment stream, or an explicit manual review pass Project field if a
future tracker schema adds one.
Manual Gemini or operator-supplied review notes are wrapped by `review pass` or
`review reject` into a `## Jade Symphony Agent Review Run` timeline comment;
label the inner note as manual evidence so operators can distinguish it from
automatic `review loop` pass evidence.

After a human accepts work and moves the issue to `Merging`, use the guarded
merge lane:

```bash
cargo run -- merge once workflows/jade-symphony.md --dry-run
cargo run -- merge once workflows/jade-symphony.md --write
```

The merge lane should:

- inspect only `Merging` issues;
- require exactly one verified linked PR;
- check PR state, base branch, checks, review/approval signal, and mergeability;
- merge clean approved work;
- safely update `BEHIND` PR branches and leave the issue in `Merging` for the
  next retry;
- route dirty or failing work to `Need Human Input` with a standalone merge
  timeline comment unless a future command can prove safe merge-lane-only
  repair;
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
cargo run -- project inspect workflows/jade-symphony.md '#<issue>'
cargo run -- doctor workflows/jade-symphony.md
```

## Recovery

When something goes wrong:

- use `cargo run -- project inspect workflows/jade-symphony.md '#<issue>'` to refresh
  tracker state;
- use `cargo run -- doctor` to find project, claim, and runtime-state
  invariant violations;
- use `cargo run -- doctor repair ISSUE` to inspect safe, uncertain, and
  dangerous repair choices before mutating tracker state;
- inspect runtime state under the configured logs root when the repair output
  says resume or reset needs operator confirmation;
- continue existing issue branches/PRs for rework rather than creating duplicate
  branches;
- keep one issue, one branch, one PR.

If a branch is dirty because `main` advanced, repair the existing PR branch,
record merge/rework timeline evidence, and rerun verification before merging.

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
