# Dream Topic: Final Parity Audit

## Theme

After the earlier Dream rounds, the remaining parity table still had a few
partial rows. This topic separates uncovered Backlog-worthy gaps from rows that
are already covered by current issues or should stay delayed.

## Evidence Anchors

- `docs/implementation_notes.md`: lists prompt rendering, tracker adapters,
  workspace/SSH, workflow reload, worker supervision, runtime state, token
  accounting, dynamic tools, status surface, and optional API observability as
  partial or delayed.
- `docs/dogfood-readiness.md`: confirms full Liquid compatibility remains a
  prompt-rendering gap and runtime state/resume wiring remains incomplete.
- `docs/bootstrap/references/openai-symphony/SPEC.md`: requires strict
  Liquid-compatible prompt rendering and records attempt/retry/observability
  fields used by runtime state and resume logic.
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/orchestrator.ex`:
  holds running entries, retry metadata, worker host/workspace paths, and
  completion totals inside the orchestrator runtime.

## Candidate Triage

### Liquid-Compatible Prompt Rendering

- Backlog seed: #326
- Dream confidence: Medium
- Why kept: no open Backlog/Todo item covered the explicit Liquid-compatible
  prompt rendering gap. The current subset is useful, but parity needs a careful
  compatibility plan.

### Runtime State Resume Wiring

- Backlog seed: #327
- Dream confidence: Medium
- Why kept: #324 covers worker supervision policy, but not the durable
  transition-level state/resume semantics needed to reconcile process restarts,
  interruptions, session registry, workpads, tracker claims, and event logs.

## Existing Coverage Checked

- #313 covers review-loop status; it should not absorb all runtime status work.
- #308 covers artifact namespace confusion.
- #321 covers app-server continuation and runtime telemetry.
- #322 covers tracker-scoped dynamic tool execution.
- #323 covers workflow reload.
- #324 covers persistent worker supervision.
- #325 covers remote SSH worker workspaces.

## Coverage Decision

The final audit created two seeds and stopped. Additional seeds for tracker
adapter smoke tests, persistent API/dashboard, or status snapshot feed would be
premature until the newly seeded runtime/app-server/supervision work is
discussed through Issue Forge.

## Lane-Authority Note

This topic log is advisory only. Main, Review, Merge, and Doctor should not
treat it as an invariant unless a seed is later promoted into an issue contract
or a repo-owned doc/skill/CLI check.
