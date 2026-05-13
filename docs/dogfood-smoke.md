# Controlled Dogfood Smoke

This is the supervised live smoke path for checking whether Jade Symphony is
ready to operate against GitHub Project v2 without manual per-issue prompting.

The smoke is intentionally opt-in. It does not create or merge production work
by itself.

## Controlled Issue

Create or select exactly one non-production issue that:

- is in the configured GitHub Project v2 tracker;
- has Project `Status` set to `Todo`;
- has label `dogfood-smoke` or a title containing `[dogfood-smoke]`;
- passes the Issue Quality Gate;
- uses a dry-run or harmless backend command;
- has an expected outcome of reaching `Agent Review`, not `Human Review`.

## Preflight

```bash
cargo run -- dogfood-smoke examples/github-project-workflow.md --dry-run
```

The report includes:

- tracker mode and fixture status;
- controlled smoke candidate count;
- executable controlled candidate count;
- runtime state path;
- event log root;
- integration gaps.

## Supervised Live Tick

When the preflight reports one executable controlled candidate, no integration
gaps, and a non-fixture tracker mode, run one bounded live tick:

```bash
cargo run -- run-loop examples/github-project-workflow.md --max-iterations 1 --write
```

Expected result:

- the controlled issue is claimed to `In Progress`;
- an isolated workspace/branch is prepared;
- the configured backend runs;
- event logs and runtime state evidence are written;
- PR handoff evidence is recorded in the issue workpad;
- main-agent completion stops at `Agent Review`.

Do not run the merge lane for this smoke unless a human has separately approved
the issue for `Merging`.

## Skip Conditions

Skip the live tick and keep the smoke at preflight-only if:

- `gh auth status` is not healthy;
- required Project fields are unavailable;
- there is not exactly one controlled smoke candidate;
- the candidate does not pass the Issue Quality Gate;
- the backend would consume a real operator session unexpectedly;
- usage limits or credentials are currently unstable.
