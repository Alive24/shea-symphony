# Performance

Status: Draft

## Focus

This milestone is concerned with non-LLM control-plane slowness.

Likely slow paths:

- repeated Project reads;
- repeated `gh` subprocess calls;
- repeated worktree status scans;
- App refreshes that trigger heavyweight command paths or eager artifact reads;
- registry/log/read-surface aggregation;
- duplicate tracker readback after small transitions.

The strongest subjective pain point is App refresh and the overall control
plane delay after LLM work has already completed. Waiting should usually mean
waiting on an LLM or external service, not waiting on repeated local
orchestration work. Temporal should own durable orchestration instead of a
custom loop.

## Initial Targets

Use relative targets until the first timing pass lands:

- SQLite-backed dashboard materialized refresh;
- Temporal Query-backed issue detail refresh;
- no mutating command from App refresh;
- App refresh should not run heavyweight Activity or tracker mutation paths;
- dashboard refresh should load artifact detail lazily after the operator drills
  down;
- top-level dashboard refresh should not read worktree path, branch detail,
  full traces, or full artifact bodies;
- no repeated Project full scan inside one workflow step unless a write requires
  readback;
- no dashboard-wide Temporal/tracker/filesystem fanout on ordinary render;
- non-LLM paths should be seconds-scale unless waiting on external services;
- status snapshots should explain external waits.

## Measurement Points

- `project state`
- `doctor`
- Temporal workflow query
- SQLite local read-model query
- Temporal Activity duration and retries
- Main lane claim
- Review lane claim/result application
- Merge lane PR read/mergeability check
- App runtime snapshot refresh

## Pending Decisions

See `QUESTIONS.md` entries `Q2607-03` and `Q2607-04`. Measure which local
operations dominate App refresh after artifact reads are removed before fixing
the first budgets or timing-event schema.
