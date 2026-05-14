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

## Inspect And Resume

```bash
target/debug/jade-symphony inspect examples/github-project-workflow.md
target/debug/jade-symphony run-loop examples/github-project-workflow.md --max-iterations 1 --write
```

For supervised parallel operators, pass `--pool N` to preview or claim multiple
eligible slots. Main work uses the `Main Agent` Project field and merge work
uses the `Merging Agent` Project field as soft claim-lock hints, so separate
Codex sessions can avoid selecting work already claimed by another session.

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
