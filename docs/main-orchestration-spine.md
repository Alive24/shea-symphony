# Main Orchestration Spine

`src/main.rs` stays the binary entrypoint and top-level dispatcher. It should
not become the default home for every new command family or lane incident fix.
New orchestration code should follow this spine:

```text
Issue Contract -> Lane Decision -> Runtime Attempt -> Evidence -> State Transition -> Recovery/Doctor
```

## Responsibility Map

- `Issue Contract`: Issue Forge, Issue Quality Gate, dependency and topology
  checks, and prompt context assembly.
- `Lane Decision`: Main, Review, Merge, and Human Review authority boundaries,
  lane claim selection, and write-mode eligibility.
- `Runtime Attempt`: workspace preparation, backend/session startup, app-server
  or tmux execution, progress heartbeats, and runtime state.
- `Evidence`: workpad updates, timeline comments, event-log records, PR handoff
  summaries, and review ledgers.
- `State Transition`: Project status and lane-claim mutations, PR link/readiness
  checks, and evidence-before-transition ordering.
- `Recovery/Doctor`: interrupted runtime reconciliation, merge repair,
  canonical-checkout checks, Doctor diagnostics, and safe repair commands.

## Current Boundaries

The first extracted boundary is `src/cli.rs`. It owns the raw Clap command
surface, grouped command aliases, help text, flag normalization, and conversion
from parsed arguments into the internal `Command` dispatcher model.

Command execution now lives under `src/commands/` when a command family has a
clear owner:

- `src/commands/forge.rs`: Issue Forge create, validate, promote, and rework
  execution. It preserves quality-gate checks, Project write ordering, readback
  verification, and rework evidence-before-status behavior.
- `src/commands/project.rs`: Project read and write command execution for
  state, issue, inspect, set-state, workpad, link-pr, add, and
  timeline-comment. It keeps write intent checks, recovery-aware mutations, and
  readback-oriented output close to the Project command family.
- `src/commands/workspace.rs`: Workspace command execution for discovery,
  adoption, ensure, and cleanup. It owns command-level worktree safety checks
  and evidence writes, while shared lane handoff planning remains outside the
  command module.
- `src/commands/doctor.rs`: Doctor command execution, repair entrypoints,
  selective issue hydration, and command-level Doctor diagnostics. It keeps
  repair suggestions and write-mode repair evidence close to the Doctor surface
  while exposing read-only summaries for debug and autopilot preflight.

`src/main.rs` still owns:

- `main()` and `run()`;
- the internal `Command` enum and dispatch match;
- concrete command execution functions that have not yet been extracted;
- lane orchestration helpers that touch tracker, workspace, runtime, or
  evidence state.

Lane execution now lives under `src/lanes/` when a lane boundary has a clear
runtime owner:

- `src/lanes/claim.rs`: shared lane claim primitives used by Main and Merge
  workers: worker identity, Project claim field names, claimability checks, and
  pool selection. It owns selection rules only, not Project mutations or runtime
  transitions.
- `src/lanes/main_loop.rs`: outer Main loop command execution and
  `RunLoopOptions`. It owns loop iteration, dry-run rendering, concurrency-slot
  selection, and delegation into existing runtime/handoff helpers while those
  helpers are still being extracted.
- `src/lanes/merge.rs`: Merge once/loop execution, merge queue selection,
  merge-agent conflict repair, merge-specific evidence, and done-state issue
  closure ordering. It keeps timeline-comment evidence before state mutation
  and keeps repaired dirty PRs in `Merging` for a later readback tick.
- `src/lanes/review/`: Review lane command execution. `status.rs` owns
  freshness and status reporting, `manual.rs` owns manual Review
  claim/pass/reject routing, and `automatic.rs` owns fake/once/loop runs,
  review-ledger writes, terminal claim evidence, checklist updates, and
  evidence-before-transition ordering.

This keeps command parsing reviewable without mixing it with Project mutation,
lane routing, runtime recovery, or workpad rendering.

## Preferred Next Extractions

- Split large lane modules only when the submodule boundary is obvious
  (`review` status/manual/loop, `merge` repair/evidence), not as loose helper
  piles.
- Move Main loop command execution after the lane modules settle; keep
  runtime/session recovery helpers grouped with the lane that owns the state
  transition.
- Keep future `LanePolicy` or `WorkerProfile` abstractions as follow-up design
  work; do not introduce them as part of a mechanical extraction.

When adding a new command family, prefer adding parser shape to `src/cli.rs` and
putting execution behavior in the smallest module that matches the spine stage
above. Add to `src/main.rs` only when the code is genuinely entrypoint or
top-level dispatch glue.
