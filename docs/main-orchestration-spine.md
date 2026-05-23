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

This keeps command parsing reviewable without mixing it with Project mutation,
lane routing, runtime recovery, or workpad rendering.

## Preferred Next Extractions

- Move Review and Merge command execution glue only as lane-specific modules,
  preserving their current authority boundaries and transition ordering.
- Move Main loop command execution only after the smaller command families are
  stable; keep runtime/session recovery helpers grouped with the lane that owns
  the state transition.
- Keep future `LanePolicy` or `WorkerProfile` abstractions as follow-up design
  work; do not introduce them as part of a mechanical extraction.

When adding a new command family, prefer adding parser shape to `src/cli.rs` and
putting execution behavior in the smallest module that matches the spine stage
above. Add to `src/main.rs` only when the code is genuinely entrypoint or
top-level dispatch glue.
