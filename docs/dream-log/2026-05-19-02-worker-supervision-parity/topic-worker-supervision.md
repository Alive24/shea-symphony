# Dream Topic: Worker Supervision Parity

## Theme

OpenAI Symphony distinguishes backend-specific failures from the general
orchestrator responsibility to supervise running workers, reconcile tracker
state, and schedule retries. Jade has several first slices, but the ownership
boundary is still spread across bounded loops, debug output, retry metadata, and
session registry evidence.

## Evidence Anchors

- `docs/bootstrap/references/openai-symphony/SPEC.md`: sections 7 and 8 define
  worker exit triggers, retry timer handling, active-run reconciliation,
  stall-timeout termination, and startup terminal workspace cleanup.
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/orchestrator.ex`:
  monitors worker process exits, schedules a continuation check on normal exit,
  schedules exponential backoff for abnormal exit, and restarts stalled entries.
- `docs/bootstrap/references/openai-symphony/elixir/test/symphony_elixir/orchestrator_status_test.exs`:
  covers retry backoff rows and stalled-worker restart behavior.
- `docs/implementation_notes.md`: marks retry timers, stall detection, and
  worker supervision as Partial.
- `docs/dogfood-readiness.md`: says Jade lacks long-running worker supervision,
  automated stall restart, full multi-worker runtime resume reconciliation, and
  persistent background review worker supervision.

## Candidate Triage

### Persistent Worker Supervision Boundary

- Backlog seed: #324
- Dream confidence: Medium
- Why kept: existing issues cover review-loop retry storms, Gemini health,
  progress heartbeat, and app-server continuation, but not the broader
  orchestrator ownership boundary for running workers and retry/stall lifecycle.
- Promotion path: Issue Forge should choose one first runtime-supervision slice,
  such as active-run reconciliation, stall restart, retry timer ownership, or
  process-exit classification.

## Existing Coverage Checked

- #305: review-loop retry storm after backend unavailable.
- #312: Gemini backend health-aware review-loop retry/wait policy.
- #318: long-running command heartbeat UX.
- #321: app-server continuation, max-turn, token/rate-limit, and runtime
  snapshot parity.
- #307 and #313: registry evidence and review-loop operator status surfaces,
  adjacent but not the worker lifecycle owner.

## Coverage Decision

One seed is enough for this round. Splitting stall restart, retry timers,
process-exit classification, and active-run reconciliation into separate
Backlog issues would likely create noise before Issue Forge chooses the first
runtime boundary.

## Lane-Authority Note

This topic log is advisory only. Main, Review, Merge, and Doctor should not
treat it as an invariant unless the seed is later promoted into an issue
contract or a repo-owned doc/skill/CLI check.
