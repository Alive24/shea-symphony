# Operator Dogfood Launcher

Use `scripts/jade-dogfood` for supervised local dogfood runs. It is a thin
operator entrypoint around the built `jade-symphony` binary and the GitHub
Project workflow.

It is intentionally not a daemon and does not hide write mode.

## Build

```bash
cargo build
```

## Preview

```bash
scripts/jade-dogfood --dry-run
```

The launcher checks:

- workflow file exists;
- `target/debug/jade-symphony` exists and is executable;
- current directory is inside a git repository;
- `gh` exists;
- `gh auth status` succeeds;
- the workflow validates.
- in write mode, the controlled dogfood smoke preflight passes.

The canonical supervised operator workflow is `workflows/jade-symphony.md`. It
defaults durable worktrees, logs, and runtime artifacts under
`~/.jade-symphony/artifacts`; set
`JADE_SYMPHONY_ARTIFACT_ROOT` before running commands to move the whole local
artifact tree.

The workflow file is an index/config, not a single prompt for every role. It
references lane prompt contracts under `workflows/prompts/`:

- `main-agent.md` for implementation ticks that stop at `Agent Review`;
- `review-agent.md` for independent review and review evidence;
- `merge-agent.md` for guarded `Merging` land/rework decisions.

Fixture workflows can still use inline prompt bodies. If the canonical workflow
declares lane prompts, all three lane paths must exist before agent
initialization continues.

After preflight, dry-run mode executes:

```bash
target/debug/jade-symphony run-loop workflows/jade-symphony.md --max-iterations 1 --dry-run
```

For a more scannable operator view, keep the same dry-run boundary and opt into
the terminal panel:

```bash
target/debug/jade-symphony run-loop workflows/jade-symphony.md --max-iterations 1 --dry-run --display tui
```

The panel view is not a full-screen dashboard. It keeps plain text and JSON/log
evidence available by default, and only changes output when `--display tui` is
passed. The same opt-in display flag is available on `project-state` and
`doctor`.

The first slice follows the current OpenAI Codex CLI terminal direction checked
against `openai/codex` on 2026-05-15: the Codex TUI crate depends on `ratatui`
and `crossterm`, with workspace versions `ratatui 0.29.0` and `crossterm
0.28.1`. Jade Symphony uses that stack for the presentation foundation while
deliberately avoiding full-screen interaction in this issue.

## Supervised Write Tick

```bash
scripts/jade-dogfood --write --confirm-write --max-iterations 1
```

Write mode is intentionally bounded. It runs one `run-loop` tick only after the
explicit confirmation flag is present. Before that mutating tick, the launcher
runs:

```bash
target/debug/jade-symphony dogfood-smoke workflows/jade-symphony.md --dry-run
```

If the smoke preflight fails, the launcher exits before claiming tracker work.

## Review Backend Setup

For live Agent Review, make the Gemini command visible to the worker process.
`review.gemini_command` is launched directly, so `gemini` is resolved from the
worker `PATH`, not from an interactive shell profile.

Prefer an absolute path when supervising review workers:

```bash
command -v gemini
```

Then configure the workflow or operator environment with that path before
running review automation:

```yaml
review:
  backend: gemini-cli
  gemini_command: /opt/homebrew/bin/gemini
```

```bash
target/debug/jade-symphony review-loop workflows/jade-symphony.md --max-iterations 1 --write
```

If Gemini cannot start, the review workpad should name the configured command,
whether worker `PATH` could resolve it, the required operator action, and the
retry command. Do not move an issue to `Human Review` unless the Review Agent
actually records passing review evidence.

Use `workflows/jade-symphony.md` for supervised review workers. Do not keep the
active review workflow only under `/tmp` or `/private/tmp`; the CLI prints
`workflow_warning=temporary_path` for those workflow files so operators can
promote reusable config into the repo.

## Inspect And Resume

```bash
target/debug/jade-symphony inspect workflows/jade-symphony.md
target/debug/jade-symphony project-state workflows/jade-symphony.md
target/debug/jade-symphony project-state workflows/jade-symphony.md --display tui
target/debug/jade-symphony doctor workflows/jade-symphony.md --display tui
target/debug/jade-symphony run-loop workflows/jade-symphony.md --max-iterations 1 --write
```

Use `project-state` before claiming work when multiple operators are active. A
healthy read prints `project_state_access=ok`, `trusted=true`, the issue count,
and a state summary. A failed read prints `project_state_access=blocked`,
`trusted=false`, and a `failure_kind` such as `auth`, `network`, `rate_limit`,
`schema`, `partial_response`, or `payload`; treat that as a blocker, not as an
empty queue.

## Artifact Root Migration

To move local runtime artifacts without changing repo-owned workflow files, set
one environment variable before launching dogfood commands:

```bash
export JADE_SYMPHONY_ARTIFACT_ROOT="$HOME/.jade-symphony/artifacts"
```

The live operator workflow derives implementation and review worktree/log paths
from that root. Existing temp Markdown files should be classified before
cleanup: normal operator workflow config belongs in `workflows/`, fixtures and
reference examples belong in `examples/`, reusable operator prompts belong in
`docs/`, issue and PR drafts belong in tracker/workpad or log artifacts, and
disposable scratch can be removed only through a separate cleanup decision.

Use the grouped `clean` surface for local cleanup and persistence questions:

```bash
target/debug/jade-symphony clean plan workflows/jade-symphony.md
target/debug/jade-symphony clean audit workflows/jade-symphony.md
```

`clean plan` is the grouped form of the existing read-only cleanup plan, while
`clean audit` classifies configured artifact/workspace residue as
`promote_to_repo`, `attach_to_tracker`, `safe_to_keep`, `cleanup_candidate`, or
`needs_human_decision`. Keep `doctor` for tracker/runtime invariants and stuck
workflow states.

For supervised parallel operators, pass `--pool N` to preview eligible slots and
apply lane-specific claim checks. Main work uses the `Main Agent` Project field
as a soft claim-lock hint while still processing one active runtime issue per
loop tick. Merge work uses the `Merging Agent` Project field and can process
multiple guarded merge slots in one bounded loop.

Operator commands also print compact `Latest:` lines for the current lane,
issue, category, action, actor, workspace/branch when known, and next expected
step. Treat these as the glanceable status bar; detailed line logs and JSONL
events remain the durable audit trail.

```bash
target/debug/jade-symphony run-loop workflows/jade-symphony.md --max-iterations 1 --pool 2 --dry-run
target/debug/jade-symphony merge-loop workflows/jade-symphony.md --max-iterations 1 --pool 2 --dry-run
```

## Logical Actor Audit

Dogfood can run many local workers through the same GitHub account. GitHub will
show the configured account for API mutations, so Jade Symphony also writes a
local `tracker_mutation` audit record to the configured event log. The record
captures the logical actor role and label, git author when configured, command,
mutation type, issue, target, from/to state when known, reason, and timestamp.

Use this audit trail to distinguish `main_agent`, `review_agent`,
`merge_agent`, operator repair, and Issue Forge activity without requiring
multiple GitHub users or tokens. Audit records must not contain secrets; token
or authorization-shaped text is redacted before serialization.

## Cleanup Planning

Cleanup planning is read-only:

```bash
target/debug/jade-symphony clean plan workflows/jade-symphony.md
target/debug/jade-symphony clean audit workflows/jade-symphony.md
target/debug/jade-symphony cleanup-plan workflows/jade-symphony.md
```

`clean plan` reports terminal worktrees that appear removable only when tracker
state is terminal, the linked PR is merged or closed, the local worktree branch
matches the issue branch, and the worktree is clean. `cleanup-plan` remains a
compatibility path for the same read-only behavior.

`clean audit` classifies local artifact and workspace residue by persistence
need. It never deletes files.

Do not use this launcher to bypass Agent Review, Human Review, or Merging role
boundaries.
