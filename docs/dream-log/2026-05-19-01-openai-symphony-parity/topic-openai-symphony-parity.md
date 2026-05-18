# Dream Topic: OpenAI Symphony Runtime Parity

## Theme

The OpenAI Symphony specification and Elixir reference implementation expose a
set of runtime behaviors that Jade Symphony has partially prepared for but has
not yet made durable in the polling runtime.

## Evidence Anchors

- `docs/bootstrap/references/openai-symphony/SPEC.md`: requires dynamic reload
  with last-known-good workflow behavior, app-server startup, continuation turns,
  `agent.max_turns`, token/rate-limit counters, and observability updates.
- `docs/bootstrap/references/openai-symphony/elixir/WORKFLOW.md`: configures
  `codex ... app-server`, `max_turns: 20`, and a Linear `linear_graphql` tool
  expectation.
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/orchestrator.ex`:
  models worker status, retry/continuation scheduling, token accounting, and
  runtime snapshots.
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/workflow_store.ex`:
  preserves the last known good workflow across invalid reload attempts.
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/codex/dynamic_tool.ex`:
  executes tracker-scoped `linear_graphql` requests in the reference.
- `docs/implementation_notes.md`: marks full Codex app-server protocol,
  runtime workflow reload, token/rate-limit accounting, dynamic tool execution,
  worker supervision, and optional API observability as partial or delayed.
- `docs/dogfood-readiness.md`: says the subprocess backend refuses
  `codex app-server`, dynamic-tool execution is not implemented, runtime reload
  wiring is not implemented, and persistent/remote web/API service mode is not
  implemented.

## Candidate Triage

### Codex App-Server Continuation Parity

- Backlog seed: #321
- Dream confidence: Medium
- Why kept: the reference treats app-server continuation, token/rate-limit
  accounting, and max-turn behavior as orchestration fundamentals, while Jade
  currently has a subprocess backend plus a first-slice event normalizer.
- Promotion path: Issue Forge should choose the first bounded slice, likely a
  protocol/fixture/runtime-snapshot slice before any unattended execution
  promise.

### Tracker-Scoped Dynamic Tool Execution

- Backlog seed: #322
- Dream confidence: Medium
- Why kept: Jade has a registry descriptor for dynamic tools, but the reference
  has executable `linear_graphql` behavior. The scope needs tracker authority
  boundaries because Jade's tracker is GitHub Project v2, not Linear.
- Promotion path: Issue Forge should decide whether the first slice is a
  descriptor-to-executor seam, a GitHub-project equivalent, or an explicit
  unsupported-tool diagnostic.

### Last-Known-Good Workflow Reload

- Backlog seed: #323
- Dream confidence: High
- Why kept: both the spec and current Jade notes already point to this exact
  runtime gap. The missing behavior is narrow enough to seed cleanly without
  promoting.
- Promotion path: Issue Forge should resolve the runtime surfaces that own
  reload checks, invalid-reload diagnostics, and status/debug reporting.

### Persistent Observability API

- Backlog seed: none
- Dream confidence: Watchlist
- Why deferred: the reference has a Phoenix API/dashboard, but Jade explicitly
  says terminal/status and JSONL come first. `status serve --once` exists as a
  local inspection bridge, so a persistent service should wait until runtime
  snapshots are live-fed by worker orchestration.

## Existing Coverage Checked

- Open Backlog currently includes #299, #305, #306, #307, #308, #316, #317,
  #318, #319, #320, #321, #322, and #323.
- #319 covers Codex config-migration prompts in tmux lanes, not app-server
  protocol parity.
- #320 covers repo-owned skill command example validation, not runtime reload or
  dynamic tool execution.
- #305, #312, and #318 cover retry storms, retry policy, and long-running UX,
  so worker supervision needs a separate duplicate check before any later seed.
- #313/#318 are adjacent to status/heartbeat concerns, which is part of why the
  persistent observability API stayed Watchlist.

## Lane-Authority Note

This topic log is advisory only. Main, Review, Merge, and Doctor should not
treat it as an invariant unless a candidate is later promoted into an issue
contract or a repo-owned doc/skill/CLI check.
