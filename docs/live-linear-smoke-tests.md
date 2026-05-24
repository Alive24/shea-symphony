# Live Linear Smoke Tests

These integration tests are opt-in checks for the normalized Linear tracker
adapter. They are read-only in this slice, and ordinary `cargo test` skips them
without credentials.

## Prerequisites

- `curl` is installed.
- `LINEAR_API_KEY` is set.
- `SHEA_LINEAR_PROJECT_SLUG` names a Linear project visible to that token.

No tokens or secrets are written to the generated workflow file. The workflow
uses `$LINEAR_API_KEY`, which Shea Symphony resolves at runtime.

## Run

```bash
SHEA_LIVE_LINEAR_SMOKE=1 \
LINEAR_API_KEY=... \
SHEA_LINEAR_PROJECT_SLUG=your-project-slug \
cargo test --test live_linear_smoke
```

The smoke runs:

- `shea-symphony project inspect <temporary-linear-live-workflow.md> '#<issue>'`

## Expected Behavior

- Without `SHEA_LIVE_LINEAR_SMOKE=1`, the test prints a skip message and passes.
- With the flag set, missing `LINEAR_API_KEY` or `SHEA_LINEAR_PROJECT_SLUG`
  fails immediately.
- The smoke must not create, update, comment on, or transition Linear issues.

## Follow-Ups

Mutation smoke tests for state updates, workpad comments, follow-up creation,
and project assignment should remain separate opt-in slices with stronger
guardrails.
