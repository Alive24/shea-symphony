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

After preflight, dry-run mode executes:

```bash
target/debug/jade-symphony run-loop examples/github-project-workflow.md --max-iterations 1 --dry-run
```

## Supervised Write Tick

```bash
scripts/jade-dogfood --write --confirm-write --max-iterations 1
```

Write mode is intentionally bounded. It runs one `run-loop` tick only after the
explicit confirmation flag is present. Before that mutating tick, the launcher
runs:

```bash
target/debug/jade-symphony dogfood-smoke examples/github-project-workflow.md --dry-run
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
target/debug/jade-symphony review-loop examples/github-project-workflow.md --max-iterations 1 --write
```

If Gemini cannot start, the review workpad should name the configured command,
whether worker `PATH` could resolve it, the required operator action, and the
retry command. Do not move an issue to `Human Review` unless the Review Agent
actually records passing review evidence.

## Inspect And Resume

```bash
target/debug/jade-symphony inspect examples/github-project-workflow.md
target/debug/jade-symphony run-loop examples/github-project-workflow.md --max-iterations 1 --write
```

For supervised parallel operators, pass `--pool N` to preview eligible slots and
apply lane-specific claim checks. Main work uses the `Main Agent` Project field
as a soft claim-lock hint while still processing one active runtime issue per
loop tick. Merge work uses the `Merging Agent` Project field and can process
multiple guarded merge slots in one bounded loop.

```bash
target/debug/jade-symphony run-loop examples/github-project-workflow.md --max-iterations 1 --pool 2 --dry-run
target/debug/jade-symphony merge-loop examples/github-project-workflow.md --max-iterations 1 --pool 2 --dry-run
```

## Cleanup Planning

Cleanup planning is read-only:

```bash
target/debug/jade-symphony cleanup-plan examples/github-project-workflow.md
```

It reports terminal worktrees that appear removable only when tracker state is
terminal, the linked PR is merged or closed, the local worktree branch matches
the issue branch, and the worktree is clean. It never deletes files.

Do not use this launcher to bypass Agent Review, Human Review, or Merging role
boundaries.
