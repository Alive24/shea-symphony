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
surface, grouped command aliases, help text, flag normalization, and the
internal `Command` dispatcher model produced from parsed arguments.

Command execution now lives under `src/commands/` when a command family has a
clear owner:

- `src/commands/autopilot.rs`: read-only `autopilot plan` execution. It owns
  the preflight snapshot shape, lane proposals, parked queue summaries,
  readiness rendering, and the explicit no-mutation planning contract.
- `src/commands/autopilot/lanes.rs`: read-only autopilot lane proposals. It
  owns Main, Review, and Merge dry-run selection, proposed next actions, and
  lane evidence summaries without mutating tracker state.
- `src/commands/autopilot/readiness.rs`: read-only autopilot readiness and
  safety summary assembly. It owns Doctor/canonical-checkout/runtime blockers
  and warnings while leaving lane proposal selection in `autopilot.rs`.
- `src/commands/clean.rs`: artifact cleanup plan and audit command execution.
  It keeps cleanup rendering, artifact-class paths, and read-only audit output
  together while leaving actual workspace cleanup under `workspace`.
- `src/commands/forge.rs`: Issue Forge create, validate, promote, and rework
  execution. It preserves quality-gate checks, Project write ordering, readback
  verification, and rework evidence-before-status behavior.
- `src/commands/forge/create.rs`: `forge create` execution. It owns dry-run
  rendering, duplicate-title protection, assignee gating, Project add/field
  writes, create readback, and script-friendly success output.
- `src/commands/forge/rework.rs`: `forge rework` execution. It owns Human
  Review source validation, active lane-claim blockers, replacement body
  readback, evidence comments, and final Rework status mutation ordering.
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
- `src/commands/project.rs`: Project command family shell and read surfaces for
  state, issue, and inspect. It owns readback-oriented output, state filtering,
  and state-summary rendering while delegating mutation glue to `write.rs`.
- `src/commands/project/write.rs`: Project mutation command glue. It owns
  set-state, workpad, timeline-comment, link-pr, and add-to-project write
  intent checks, recovery keys, mutation audit records, and dry-run output.
- `src/commands/session.rs`: Session startup/list/attach execution plus manual
  lane-claim command glue. It owns structured manual claim creation, session
  prompt artifact paths, registry evidence, and session-start workpad/timeline
  evidence while exposing the small lane/session helpers still shared by Review
  and Merge orchestration.
- `src/commands/session/backend.rs`: Session backend selection and tmux command
  resolution for Main, Review, and Merge. It keeps backend names, fallback
  rules, and command validation together without touching claim creation or
  tracker evidence.
- `src/commands/session/claim.rs`: Manual lane-claim command execution and
  claim-to-session validation. It owns accepted claim states, active-claim
  blockers, worker labels, manual registry evidence, and structured claim
  parsing without changing session startup.
- `src/commands/session/start.rs`: `session start` execution. It owns explicit
  run validation, workspace preparation, prompt artifact paths, backend launch,
  event-log capture, and session-start workpad/timeline evidence.
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
- `src/orchestration/session_status.rs`: shared read-only session status
  snapshots and probe defaults used by status, debug, Doctor, clean, autopilot,
  and lane runtime modules.
- `src/orchestration/canonical_checkout.rs`: command-level canonical checkout
  guard/report glue. It owns write-mode refresh output, dry-run preview output,
  and Project/Doctor integration-gap reporting while the lower-level checkout
  scanner stays in the library module.
- `src/orchestration/tracker_context.rs`: shared tracker context helpers:
  backend labels for progress output, live GitHub detection, issue evidence
  hydration, and configured Project state lists used across commands and lanes.
- `src/orchestration/time.rs`: shared runtime clock and GMT timestamp helpers
  used in session evidence, Forge/Rework notes, audit records, and runtime
  state updates.
- `src/orchestration/progress.rs`: command/lane progress heartbeat spec
  builders. It attaches actor identity and the shared event-log path while the
  heartbeat implementation and stdout/stderr safety live in the library
  `src/progress.rs`.
- `src/orchestration/text.rs`: shared human-output formatting helpers for
  single-line evidence summaries and shell command display strings.
- `src/orchestration/workflow_config.rs`: workflow config loading, temporary
  workflow path operator warnings, and explicit write-intent gating shared by
  commands and lanes.

Tracker adapter code should stay backend-oriented instead of accumulating in a
single catch-all file:

- `src/tracker.rs`: shared tracker trait, tracker factory, GitHub Project v2
  adapter methods, shared normalization/parsing helpers, and fixture loading
  used by tracker backends.
- `src/tracker/error.rs`: shared tracker error type plus Project-state failure
  classification used by CLI readback, retry handling, doctor output, and tests.
- `src/tracker/follow_up.rs`: shared follow-up issue input shape and body
  rendering used by GitHub and Linear backends.
- `src/tracker/memory.rs`: in-memory fixture tracker backend for dry-run and
  test surfaces.
- `src/tracker/project_field.rs`: shared Project field assignment parser for
  `NAME=VALUE` CLI inputs used by Project and lane claim mutations.
- `src/tracker/state.rs`: shared tracker state normalization and read filters:
  claim decisions, mapped-status checks, assignee filtering, and no-op status
  update detection used by backends and lane selection.
- `src/tracker/github/cli.rs`: GitHub CLI invocation, auth-mode detection,
  retry/timeout handling, and JSON/status response validation used by the
  Project v2 adapter.
- `src/tracker/github/client.rs`: live GitHub Project v2 adapter client. It
  owns `gh` GraphQL/REST read/write orchestration, Project owner fallback,
  metadata loading, Project item mutation, issue edits/comments, workpad
  upserts, linked-PR comments, issue closure, and targeted evidence hydration.
- `src/tracker/github/client/project.rs`: Project metadata and field-update
  support for the live GitHub client, including REST metadata/item overlays,
  GraphQL Project page transport, and REST-first Project item field mutation.
- `src/tracker/github/client/read.rs`: targeted GitHub issue evidence and
  native dependency reads used after queue scans or issue-specific lookups.
- `src/tracker/github/evidence.rs`: GitHub issue evidence parsing, including
  issue body plus workpad/timeline comment composition, blocker/native
  dependency refs, rich issue evidence merging, Project field extraction, and
  linked PR discovery.
- `src/tracker/github/project_v2.rs`: GitHub Project v2 metadata/cache types,
  owner typing, Project item and targeted issue response parsing, REST overlay
  merging, REST field update payloads, and REST project path construction.
- `src/tracker/github/queries.rs`: GitHub Project v2 GraphQL query and mutation
  documents, page-size constants, and read-mode shaping for queue scans versus
  rich issue evidence.
- `src/tracker/github/topology.rs`: native GitHub parent/subissue topology
  helpers, including subissue field insertion, missing Project-state hydration,
  and issue-ref matching used by queue and targeted reads.
- `src/tracker/linear.rs`: Linear tracker backend, including fixture/live
  dispatch, Linear GraphQL query and mutation documents, Linear issue parsing,
  state-name mapping, and Linear-specific integration-gap reporting.
- `src/tracker/workpad.rs`: shared tracker workpad marker and merge logic for
  evidence blocks that may be written by GitHub or Linear backends.
- `src/tracker/tests.rs`: tracker unit tests and test fixtures that need access
  to private parser/cache helpers while keeping production adapter code
  readable.

`src/main.rs` still owns:

- `main()` and `run()`;
- the top-level dispatch match over `cli::Command`;
- small binary-scoped helper shims and re-exports that have not yet moved to
  library or orchestration modules.

Binary integration-style tests live under `src/main/` instead of inline in the
entrypoint:

- `src/main/tests.rs`: binary test prelude and cross-surface behavior tests
  that still need access to private binary shims while extraction continues.
  Test-only imports belong here, not in `src/main.rs`.
- `src/main/tests/support.rs`: shared binary test fixtures, fake tracker
  adapter, parser helpers, git repository fixtures, and merge command runner.
  Keep support here when multiple binary test modules need the same fixture.
- `src/main/tests/autopilot.rs`: read-only autopilot plan fixtures and lane
  readiness rendering tests.
- `src/main/tests/forge.rs`: Issue Forge, promote/rework, Link PR, and Rework
  evidence-ordering behavior tests that exercise binary-private helpers.
- `src/main/tests/main_loop.rs`: Main-loop smoke gates, selection, runtime
  state shaping, pending-session reconciliation, recovery handoff reuse,
  no-dispatch actions, and write-mode guard tests.
- `src/main/tests/main_loop/handoff.rs`: Main-loop handoff plan/workpad, live
  PR linkage, handoff verification, usage-limit pause workpad, and Agent
  Review handoff evidence tests.
- `src/main/tests/main_loop/runtime.rs`: Main-loop runtime preflight and
  recovery tests, including stale-state archival, retry timing,
  active-session retention, tmux/app-server recovery candidates, and fake tmux
  helpers.
- `src/main/tests/merge.rs`: Merge session backend defaults, clean merge tick,
  merge-agent repair evidence, merge worker selection/recovery, and Done-state
  completion ordering tests.
- `src/main/tests/parser.rs`: CLI parser/help/flag compatibility tests for the
  grouped command surface produced by `src/cli.rs`.
- `src/main/tests/parser/forge.rs`: Forge, Forge Validate/Rework/Promote, and
  Link PR parser compatibility tests for the grouped command surface.
- `src/main/tests/parser/lanes.rs`: Main, Review, and Merge loop/once parser
  compatibility tests, including write/recover defaults and numeric guards.
- `src/main/tests/review.rs`: automatic and manual review command glue tests,
  review worker selection, terminal Review Agent claims, checklist update
  ordering, and review workspace placement.

Lane execution now lives under `src/lanes/` when a lane boundary has a clear
runtime owner:

- `src/lanes/claim.rs`: shared lane claim primitives used by Main and Merge
  workers: worker identity, Project claim field names, claimability checks,
  claim value rendering, recovery-aware claim-field writes, and pool
  selection. It owns claim records and claim-field mutation glue, not broader
  Project state transitions or runtime execution.
- `src/lanes/main_loop.rs`: outer Main loop command execution and
  `RunLoopOptions`. It owns one-shot Main execution, loop iteration, dry-run
  rendering, concurrency-slot selection, and delegation into narrower Main-lane
  helpers.
- `src/lanes/main_loop/dispatch.rs`: Main-loop write dispatch shell. It owns
  selected-worker logging, backend readiness summaries, concurrent worker
  spawning, and per-worker outcome aggregation while delegating the single-issue
  mutation path back to the Main-loop state machine.
- `src/lanes/main_loop/write_candidate.rs`: single-issue Main-loop write
  dispatch. It owns the claim/resume, ownership evidence, runtime state,
  backend/session reconciliation, handoff workpad, retry, and final Agent
  Review or Need Human Input state transition path for one selected issue.
- `src/lanes/main_loop/write_candidate/live_handoff.rs`: live Main handoff
  publish/link/readiness steps after a backend succeeds. It keeps commit,
  verification, PR publication, Project PR link evidence, and draft-to-ready
  handling together.
- `src/lanes/main_loop/write_candidate/terminal.rs`: terminal Main write
  candidate transitions. It owns Agent Review handoff evidence, usage-limit
  pause recording, retry scheduling, PR-linkage invariant failures, and
  Need Human Input fallback after retry exhaustion.
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
  persisted runtime-state retention, retry backoff, and stalled-state routing.
  It does not own Project mutation or Agent Review handoff evidence.
- `src/lanes/main_loop/runtime/archive.rs`: stale Main runtime-state archival.
  It owns workspace clean/dirty checks, archive reason mapping, and durable
  archive file writes for runtime states that no longer match tracker state.
- `src/lanes/main_loop/runtime/recovery.rs`: Main-loop runtime/session recovery
  policy. It owns registered-session recovery candidates, active-session
  retention decisions, terminal-session reconstruction, and recoverable session
  reason text.
- `src/lanes/main_loop/runtime/session_probe.rs`: Main runtime session probing.
  It owns session-registry record conversion, tmux/log probing, session status
  classification, and active-worker status priority.
- `src/lanes/main_loop/selection.rs`: Main-loop issue selection and claim
  decisions. It owns recover-first selection, claim/resume/replan
  classification, live GitHub assignee identity lookup/ownership checks, and
  no-dispatch idle behavior. It does not write tracker state or render handoff
  evidence.
- `src/lanes/main_loop/supervision.rs`: Main-loop runtime supervision event
  logging. It owns JSONL records tied to runtime state, active issues, backend
  sessions, profiles, instances, and actor identity; tracker mutation audit
  remains under `src/orchestration/tracker_recovery.rs`.
- `src/lanes/merge.rs`: Merge once/loop execution, merge queue selection,
  recovery selection for interrupted merge claims, stale/dirty/readiness
  routing, merge-specific evidence, and done-state issue closure ordering. It
  keeps timeline-comment evidence before state mutation and keeps repaired
  dirty PRs in `Merging` for a later readback tick.
- `src/lanes/merge/evidence.rs`: Merge-lane tracker mutation evidence. It owns
  timeline-comment recovery keys, mutation audit records, state writes, and
  Done-state issue closure ordering so evidence is recorded before transitions.
- `src/lanes/merge/repair.rs`: Merge-agent conflict repair. It owns the
  trusted repair preflight, merge-agent prompt, backend event capture, semantic
  safety markers, verification, push, and repair evidence text for dirty PRs.
- `src/lanes/merge/repair/agent_contract.rs`: Merge-agent repair prompt and
  output contract. It owns the conflict-repair prompt text, required decision
  markers, agent event text aggregation, and semantic-safety summary parsing.
- `src/lanes/merge/repair/outcome.rs`: Merge-agent repair outcomes. It owns
  successful repair finalization, push verification, and the standard blocked,
  backend-failed, semantic-uncertain, and verification-failed evidence shapes.
- `src/lanes/merge/selection.rs`: Merge worker selection and recovery
  candidates. It owns priority-ordered claim recovery, unclaimed merge-worker
  selection, and the rule that manual merge claims are not adopted by the loop.
- `src/lanes/merge/tick.rs`: Single merge-lane tick execution. It owns queue
  reads, merge claim writes, PR preflight, merge-lane decisions, evidence
  recording, and state transitions for one selected Merging issue.
- `src/lanes/merge/tick/dirty.rs`: Dirty merge decision handling. It owns the
  safe conflict repair attempt, merge-agent repair fallback, dirty-PR evidence,
  and Need Human Input routing for unrepairable dirty branches.
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
- Keep `src/commands/session.rs` as the session facade and only split more
  session helpers when a command surface or evidence owner is obvious; keep
  parser/help conversion in `src/cli.rs`.
- Keep future `LanePolicy` or `WorkerProfile` abstractions as follow-up design
  work; do not introduce them as part of a mechanical extraction.

When adding a new command family, prefer adding parser shape to `src/cli.rs` and
putting execution behavior in the smallest module that matches the spine stage
above. Add to `src/main.rs` only when the code is genuinely entrypoint or
top-level dispatch glue.
