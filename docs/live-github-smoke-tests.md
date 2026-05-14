# Live GitHub Smoke Tests

These integration tests are opt-in checks for a real GitHub Project v2
workflow. They are read-only / dry-run only in this slice, and ordinary
`cargo test` skips them without credentials.

## Prerequisites

- `gh` is installed.
- `gh auth status` succeeds for the `Alive24/jade-symphony` repository.
- The workflow at `workflows/jade-symphony.md` points at the intended
  GitHub Project v2 tracker.

No tokens or secrets are printed by the tests.

## Run

```bash
JADE_LIVE_GITHUB_SMOKE=1 cargo test --test live_github_smoke
```

The smoke runs:

- `jade-symphony inspect workflows/jade-symphony.md`
- `jade-symphony dogfood-smoke workflows/jade-symphony.md --dry-run`

## Expected Behavior

- Without `JADE_LIVE_GITHUB_SMOKE=1`, the tests print a skip message and pass.
- With the flag set, missing or unusable `gh` authentication is a test failure.
- The smoke must not mutate GitHub Project state, workpad comments, issues, or
  PRs.

## Follow-Ups

Mutation smoke tests for status updates, workpad upsert, and controlled
worktree/PR handoff should remain separate opt-in slices with their own
guardrails.
