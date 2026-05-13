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

After preflight, dry-run mode executes:

```bash
target/debug/jade-symphony run-loop examples/github-project-workflow.md --max-iterations 1 --dry-run
```

## Supervised Write Tick

```bash
scripts/jade-dogfood --write --confirm-write --max-iterations 1
```

Write mode is intentionally bounded. It runs one `run-loop` tick only after the
explicit confirmation flag is present.

## Inspect And Resume

```bash
target/debug/jade-symphony inspect examples/github-project-workflow.md
target/debug/jade-symphony run-loop examples/github-project-workflow.md --max-iterations 1 --write
```

Do not use this launcher to bypass Agent Review, Human Review, or Merging role
boundaries.
