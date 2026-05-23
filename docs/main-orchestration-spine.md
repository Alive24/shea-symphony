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

- `src/commands/autopilot.rs`: read-only `autopilot plan` execution. It owns
  the preflight snapshot shape, lane proposals, parked queue summaries,
  readiness rendering, and the explicit no-mutation planning contract.
- `src/commands/clean.rs`: artifact cleanup plan and audit command execution.
  It keeps cleanup rendering, artifact-class paths, and read-only audit output
  together while leaving actual workspace cleanup under `workspace`.
- `src/commands/forge.rs`: Issue Forge create, validate, promote, and rework
  execution. It preserves quality-gate checks, Project write ordering, readback
  verification, and rework evidence-before-status behavior.
- `src/commands/follow_up.rs`: low-level `create-follow-up` execution for
  explicit operator follow-up creation from a body file. It stays separate from
  Issue Forge so quality-gated issue creation remains easy to find.
- `src/commands/gate.rs`: quality-gate command execution plus shared gate
  evaluation helpers used by Forge, Project inspect, and Main lane dispatch.
  It keeps gate workpad rendering and gate-to-state mapping with the gate
  decision logic.
- `src/commands/profiles.rs`: execution profile listing. It owns profile
  discovery output while shared profile selection for lane execution remains
  with the lane/runtime callers.
- `src/commands/project.rs`: Project read and write command execution for
  state, issue, inspect, set-state, workpad, link-pr, add, and
  timeline-comment. It keeps write intent checks, recovery-aware mutations, and
  readback-oriented output close to the Project command family.
- `src/commands/session.rs`: Session startup/list/attach execution plus manual
  lane-claim command glue. It owns structured manual claim creation, session
  backend selection, prompt artifact paths, registry evidence, and
  session-start workpad/timeline evidence while exposing the small lane/session
  helpers still shared by Review and Merge orchestration.
- `src/commands/skills.rs`: skill readiness status rendering for the grouped
  `skills status` surface.
- `src/commands/status.rs`: read-only runtime status surfaces for `plan` and
  `status serve`. It owns snapshot assembly, JSON/human rendering, and loopback
  status serving while reusing shared session inspection.
- `src/commands/workflow.rs`: workflow validation and top-level workflow
  inspection glue. It keeps config loading, progress-wrapped Project summary
  reads, state filtering, and gate summary rendering out of the binary
  entrypoint.
- `src/commands/workspace.rs`: Workspace command execution for discovery,
  adoption, ensure, and cleanup. It owns command-level worktree safety checks
  and evidence writes, while shared lane handoff planning remains outside the
  command module.
- `src/commands/doctor.rs`: Doctor command execution, repair entrypoints,
  selective issue hydration, and command-level Doctor diagnostics. It keeps
  repair suggestions and write-mode repair evidence close to the Doctor surface
  while exposing read-only summaries for debug and autopilot preflight.
- `src/commands/debug.rs`: read-only debug report execution, Project/Doctor
  summary rendering, smoke-readiness classification, session summaries, and
  lane next-action hints. Shared app-server smoke-gate facts stay with Main
  orchestration while the command owns the report layout.

Cross-command orchestration helpers live under `src/orchestration/` when they
have a narrow shared responsibility:

- `src/orchestration/tracker_recovery.rs`: recovery-aware tracker mutations,
  mutation audit records, recovery markers, stable recovery keys, and PR-merge
  readback recovery. It is shared by command execution and lane modules because
  evidence-before-transition ordering depends on the same idempotent write
  contract in each surface.

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
  selection, and delegation into narrower Main-lane helpers.
- `src/lanes/main_loop/dispatch.rs`: Main-loop write dispatch shell. It owns
  selected-worker logging, backend readiness summaries, concurrent worker
  spawning, and per-worker outcome aggregation while delegating the single-issue
  mutation path back to the Main-loop state machine.
- `src/lanes/main_loop/dry_run.rs`: Main-loop dry-run action rendering. It owns
  the read-only preview lines for claim/resume, handoff plan, backend identity,
  worktree, verification, PR, workpad, and Agent Review handoff actions.
- `src/lanes/main_loop/execution.rs`: Main-agent backend execution for a single
  issue workspace. It owns prompt artifact persistence, backend event logging,
  hook execution, usage-limit detection, and the `IssueExecutionResult` shape
  consumed by Main-loop handoff logic.
- `src/lanes/main_loop/failure.rs`: Main-loop dispatch blocker handling. It
  owns quality-gate and handoff-plan failure output, including the existing
  workpad-before-state ordering for those blocked paths.
- `src/lanes/main_loop/handoff.rs`: Main-loop handoff planning and evidence. It
  owns issue workspace/branch handoff plans, recovery handoff repair, runtime
  ownership workpad text, configured handoff verification, live PR link
  readback, Agent Review handoff evidence, and usage-limit pause workpad text.
  It does not perform the final Project state transition.
- `src/lanes/main_loop/preflight.rs`: Main-loop backend preflight facts. It owns
  app-server smoke readiness classification and the write-mode guard that
  blocks `main loop --write` when the configured Main backend is `dry-run`.
- `src/lanes/main_loop/session.rs`: Main-loop pending-session reconciliation
  and runtime-state result shaping. It owns session registry readback,
  stale/unknown active-session classification, terminal session reconstruction,
  post-handoff session/runtime cleanup, and runtime-state transitions derived
  from backend results.
- `src/lanes/main_loop/runtime.rs`: Main-loop runtime preflight and recovery:
  persisted runtime-state retention, active session detection, recoverable
  terminal/stale session selection, retry backoff, and stale runtime-state
  archiving. It does not own Project mutation or Agent Review handoff evidence.
- `src/lanes/main_loop/selection.rs`: Main-loop issue selection and claim
  decisions. It owns recover-first selection, claim/resume/replan
  classification, live GitHub assignee ownership checks, and no-dispatch idle
  behavior. It does not write tracker state or render handoff evidence.
- `src/lanes/main_loop/supervision.rs`: Main-loop runtime supervision event
  logging. It owns JSONL records tied to runtime state, active issues, backend
  sessions, profiles, instances, and actor identity; tracker mutation audit
  remains under `src/orchestration/tracker_recovery.rs`.
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
- Split remaining session-adjacent runtime helpers only when they clearly
  belong to a lane runtime module; keep parser/help conversion in `src/cli.rs`
  and keep session execution in `src/commands/session.rs`.
- Keep future `LanePolicy` or `WorkerProfile` abstractions as follow-up design
  work; do not introduce them as part of a mechanical extraction.

When adding a new command family, prefer adding parser shape to `src/cli.rs` and
putting execution behavior in the smallest module that matches the spine stage
above. Add to `src/main.rs` only when the code is genuinely entrypoint or
top-level dispatch glue.
