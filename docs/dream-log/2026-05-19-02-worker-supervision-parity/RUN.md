# Dream Run: Worker Supervision Parity

Date: 2026-05-19
Run: `2026-05-19-02-worker-supervision-parity`
Mode: write-mode Dream continuation
Operator context: continue comparing Jade Symphony with OpenAI Symphony SPEC and Elixir reference implementation

## Source Inventory

- `git status --short --branch`
- `cargo run -- project state workflows/jade-symphony.md`
- `cargo run -- project issue workflows/jade-symphony.md '#324' --json`
- `gh issue list --repo Alive24/jade-symphony --state open --search "retry OR stall OR supervision OR continuation OR worker in:title,body" --json number,title,url,state --limit 80`
- `gh issue list --repo Alive24/jade-symphony --state open --search "worker supervision OR persistent worker OR multi-worker OR runtime resume OR stall restart" --json number,title,url,state --limit 80`
- `gh issue list --repo Alive24/jade-symphony --state open --search "runtime resume OR session registry OR retry backoff OR stall detection OR worker lifecycle" --json number,title,url,state --limit 80`
- `gh issue view 305 --repo Alive24/jade-symphony --json number,title,state,body,url`
- `gh issue view 312 --repo Alive24/jade-symphony --json number,title,state,body,url`
- `gh issue view 318 --repo Alive24/jade-symphony --json number,title,state,body,url`
- `gh issue view 321 --repo Alive24/jade-symphony --json number,title,state,body,url`
- `docs/bootstrap/references/openai-symphony/SPEC.md`
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/orchestrator.ex`
- `docs/bootstrap/references/openai-symphony/elixir/test/symphony_elixir/orchestrator_status_test.exs`
- `docs/implementation_notes.md`
- `docs/dogfood-readiness.md`

## Round Summary

This round focused on the part of the OpenAI Symphony reference that sits above
individual backend health checks: the persistent worker lifecycle. The spec
separates normal worker exit, abnormal worker exit, retry timers, active-run
tracker reconciliation, and stall timeout handling. The Elixir orchestrator
implements this as monitored worker processes that schedule continuation checks,
failure backoff, and stalled-worker restart.

Jade already has important first slices: bounded loops, retry metadata, runtime
state persistence, stall/status reporting, and operator-facing debug output. The
missing piece is the broader supervision boundary for a long-running runtime:
which component owns active-run reconciliation, process-exit classification,
stall restart, retry timers, session evidence, and release behavior.

## Created Backlog

- #324 `Backlog: define persistent worker supervision boundary`

## Watchlist / Not Created

- Review-loop retry storms: not created because #305 already covers retry storm
  suppression after backend unavailable.
- Gemini review backend health policy: not created because #312 is an
  executable open issue for health-aware wait/retry/block guidance.
- Long-running command progress heartbeat: not created because #318 already
  covers operator heartbeat UX.
- Codex app-server continuation/token/runtime telemetry: not created because
  #321 already covers that parity seed.
- Process-restart durable retry timers: kept inside #324 as a guardrail/open
  question rather than a separate seed, because the upstream spec explicitly
  says restart recovery is tracker/filesystem driven and retry timers are not
  restored from process memory.

## Doctor / Project Warnings

- `project state` after #324 reports `Agent Review:1, Backlog:14, Done:77,
  Todo:4`.
- The canonical checkout was clean after the prior Dream commit, but local main
  is ahead of `origin/main`; `project state` reports
  `canonical_checkout_blocked: local main does not exactly match origin/main`.
  This is expected after committing Dream Log artifacts locally and does not
  imply lane work should run.
- The prior Doctor warning set remains relevant: #243 terminal Review Agent
  claim missing registry evidence and local Codex/Gemini skill install drift.

## Gemini Review Status

Gemini review passed. See `gemini-review.md`.

## Slept Enough

Slept enough: no.

Reason: the runtime-supervision parity theme produced one strong seed and
resolved obvious duplicate risks, but the OpenAI Symphony reference still has a
separate theme worth checking: worker host / SSH execution and workspace
lifecycle boundaries. That should be compared against Jade's current local
worktree/session model before deciding whether another seed is needed.

## Safety Notes

Dream-created issue #324 stayed in `Backlog`. It was not promoted, claimed, or
treated as executable lane work. Project state and issue creation were verified
through the Jade Symphony CLI.
