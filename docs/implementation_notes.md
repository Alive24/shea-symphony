# Jade Symphony Implementation Notes

Status: initial implementation notes and parity roadmap.

## Source Order

Jade Symphony is implemented from these local sources, in priority order:

1. `docs/bootstrap/references/openai-symphony/SPEC.md`
2. `docs/bootstrap/references/openai-symphony/elixir/README.md`
3. `docs/bootstrap/references/openai-symphony/elixir/WORKFLOW.md`
4. `docs/bootstrap/references/openai-symphony/elixir/lib/`
5. `docs/bootstrap/JADE_SYMPHONY_SPEC.md`
6. `docs/bootstrap/TRACKER_GITHUB_PROJECT_V2.md`
7. `docs/bootstrap/JADE_WORKFLOW.md`
8. `docs/bootstrap/ISSUE_QUALITY_GATE_TEMPLATE.md`

Files under `docs/bootstrap/references/openai-symphony` are official reference
inputs and must not be edited by Jade Symphony implementation work.

## Official Feature Inventory

Normative baseline from `SPEC.md`:

- `WORKFLOW.md` loader with optional YAML front matter and trimmed prompt body.
- typed runtime config with defaults, validation, environment indirection, and
  runtime reload semantics.
- normalized issue tracker client, active-state fetch, state refresh, and
  terminal-state fetch.
- single-authority orchestrator state for polling, claims, retries, dispatch,
  reconciliation, stall handling, and runtime snapshots.
- deterministic per-issue workspaces with path sanitization, containment checks,
  lifecycle hooks, and terminal cleanup.
- strict prompt rendering with `issue` and `attempt` inputs.
- coding-agent app-server runner, Codex session/turn metadata, token/rate-limit
  accounting, and dynamic tool handling where enabled.
- structured logs with issue/session context.
- operator-visible status surface, plus optional HTTP/API observability.
- conformance tests for workflow/config, workspace safety, tracker behavior,
  orchestrator state, app-server behavior, observability, and CLI lifecycle.

Feature-parity baseline from the Elixir implementation:

- `Workflow`/`WorkflowStore`: load from explicit path or `./WORKFLOW.md`, keep
  last known good workflow on reload failures.
- `Config.Schema`: defaults for Linear tracker, polling, workspace root, worker
  SSH hosts, agent limits, Codex settings, hooks, observability, and server.
- `Tracker` facade: adapter boundary for candidate issues, state refresh, comments,
  and state transitions.
- `Linear.Client`/`Linear.Adapter`: GraphQL fetch, pagination, blockers, labels,
  assignee routing, comment creation, and state updates.
- `Workspace`: safe workspace key, root containment checks, local and SSH workers,
  `after_create`, `before_run`, `after_run`, and `before_remove` hooks.
- `AgentRunner`: workspace prep, before/after hooks, Codex session start, repeated
  turns while the issue remains active, and issue refresh between turns.
- `Codex.AppServer`: stdio app-server protocol, approvals/user-input/tool events,
  usage/rate-limit telemetry, and session lifecycle.
- `Codex.DynamicTool`: `linear_graphql` client-side tool.
- `Orchestrator`: polling, dispatch sorting, blocker checks, per-state/global
  concurrency, retry queue, continuation retry, stall restart, terminal cleanup,
  snapshot, and dashboard notifications.
- `StatusDashboard` and Phoenix API/UI: terminal status, runtime snapshot payloads,
  `/api/v1/state`, per-issue lookup, and refresh trigger.
- `LogFile`, logging docs, token-accounting docs, CLI guardrail acknowledgement,
  optional `--logs-root` and `--port`, and test-only memory tracker.

## Jade Extensions To Preserve

Jade-specific bootstrap docs require:

- GitHub Project v2 as the first concrete tracker adapter.
- Future Linear adapter preserved behind the same normalized tracker abstraction.
- normalized `TrackerIssue` model with issue ID, ProjectV2 item ID, identifier,
  state, labels, assignees, priority, linked PRs, blockers, project fields, and
  timestamps.
- assignee filtering for multi-owner dispatch.
- Issue Forge modes: `discover`, `discuss`, `draft`, `validate`, and `repair`.
- question-driven clarification loop that asks only execution-critical questions.
- Issue Quality Gate before dispatch.
- normalized states including `Need to Clarify`, `Need Human Input`, and
  `Agent Review`.
- Codex and Claude Code as peer agent backends.
- independent Agent Review before Human Review with `Confirmed`, `Plausible`,
  `Rejected`, and `Needs Context` finding classes.
- structured event log, run summary, runtime snapshot, status command/surface,
  and future-compatible API observability.

## Rust Architecture Plan

Initial crate layout:

- `workflow`: source-faithful loader for Markdown plus optional YAML front matter.
- `config`: typed settings, defaults, `$VAR` resolution, path normalization, and
  supported tracker/backend selection.
- `model`: normalized tracker issue, blockers, pull requests, states, gate
  decisions, snapshots, retries, and agent events.
- `tracker`: trait plus dry-run memory, GitHub Project v2, and Linear adapters.
  GitHub/Linear API calls stay inside adapters.
- `quality_gate`: executable-issue classifier based on the Jade issue contract.
- `issue_forge`: candidate/draft/validation structures and clarification loop
  primitives.
- `workspace`: safe workspace keys, root containment, lifecycle hooks, and future
  remote-worker extension points.
- `agent`: backend trait plus Codex, Claude Code, and dry-run backends that emit
  normalized events.
- `review`: independent agent review data model and gate summary.
- `orchestrator`: dispatch planning, state ownership, retry metadata, blocker and
  state checks, and snapshot production.
- `event_log`: structured JSONL event sink.
- `status_surface`: terminal/operator-readable snapshot rendering.
- `main`: CLI for loading config, validating, rendering status, and dry-run
  dispatch planning.

The first vertical slice intentionally avoids hard-coded GitHub Project v2 logic
inside `orchestrator`; the orchestrator only consumes `TrackerIssue` records from
the `TrackerAdapter` trait.

## Tracker Plan

GitHub Project v2 adapter:

- reads ProjectV2 items, keeps ProjectV2 status field details inside the adapter,
  and normalizes GitHub Issues into `TrackerIssue`.
- supports ProjectV2 status option lookup and update by option ID.
- supports workpad upsert through issue comments with
  `<!-- jade-symphony-workpad -->`.
- treats same-state status updates as adapter-local no-ops and exposes
  tracker-level claim decisions for `Todo`/`Rework`, active `In Progress`, and
  externally changed states.
- supports linked PR lookup/linking, follow-up issue creation, and project item
  addition.
- if GitHub credentials or network access are unavailable, dry-run fixtures remain
  usable and the integration gap is reported explicitly.

Linear adapter:

- remains a required adapter behind the same trait.
- maps issue state, description, assignee, project, blocker relations, labels,
  branch names, timestamps, and priority into `TrackerIssue`.
- supports live GraphQL reads, mapped workflow-state updates, marker workpad
  comment upsert, follow-up issue creation, and adding an issue to the
  configured project when Linear credentials are available.
- raw Linear GraphQL dynamic-tool parity is tracked separately from basic adapter
  reads/writes.

## Backend Plan

Agent backends use one normalized interface:

- `prepare(workspace, rendered_prompt, config)`
- `run()`
- `stream_events()`
- `stop(reason)`
- `summarize()`

Codex is first, Claude Code is a peer backend, and the dry-run backend is for
tests and credential-limited bootstrap work. Backend events normalize session
start, message, token usage, rate-limit, review, completion, and error records.

## Observability Plan

Initial observability:

- JSONL event log.
- runtime snapshot model with running, retrying, skipped, polling, token,
  event-log path, and integration-gap fields.
- runtime state file model under `logs_root/runtime` for active issue,
  workspace, branch, backend session, attempt count, last event, and last
  transition.
- terminal/status renderer for operator-readable snapshots.
- run summary data model.

Planned parity:

- API-compatible snapshot endpoints inspired by the Elixir Phoenix surface.
- richer token/rate-limit accounting once the Codex app-server client is active.
- dashboard refresh semantics remain a status-surface concern, not orchestrator
  business logic.

## Issue Forge And Quality Gate Plan

Issue Forge owns upstream issue creation and repair. Quality Gate owns dispatch
eligibility. Orchestrator must not dispatch a `Todo` issue until the gate returns
`Ready` or `Ready With Assumptions`; critical missing context routes to
`Need to Clarify`.

The initial gate checks for the Jade template's required sections and classifies
missing execution-critical fields. Later iterations can add source-alignment,
duplication, and tracker-write repair flows without changing orchestrator shape.

## Risks

- GitHub Project v2 GraphQL details are easy to leak into orchestration unless the
  adapter boundary is kept strict.
- A minimal vertical slice can accidentally look like completion; the parity
  roadmap below must stay visible until all baseline categories are covered.
- Codex and Claude Code protocols differ; the backend abstraction must normalize
  run events without hiding backend-specific setup or failure details.
- Prompt rendering must become fully Liquid-compatible before production use.
- Workspace hooks and path safety need strong tests before any unattended run.
- External credentials may be missing during bootstrap; dry-run mode must remain
  deterministic and honest about skipped integrations.

## Acceptance Criteria

- Local implementation notes exist outside `docs/bootstrap/references`.
- Rust crate builds and exposes the planned module boundaries.
- `WORKFLOW.md` loading, config defaults/validation, normalized issue model,
  tracker abstraction, quality gate, issue forge primitives, agent backend
  abstraction, orchestrator dispatch planning, workspace path safety, event log,
  and status surface have tests or dry-run coverage.
- Formatting and tests run locally.
- Delayed parity items are recorded with source path, current status, reason, and
  planned implementation path.

## Parity Roadmap

| Capability | Source | Status | Reason For Delay | Planned Path |
| --- | --- | --- | --- | --- |
| Full Codex app-server stdio protocol | `SPEC.md`, `elixir/lib/symphony_elixir/codex/app_server.ex` | Partial | A conservative workspace-bound Codex subprocess backend exists, and a first-slice event normalizer maps fixture JSON-RPC stream lines into Jade `AgentEvent` values. Full app-server protocol framing, request/response transport, continuation turns, and live Codex validation still require protocol-specific implementation. | Replace or extend the subprocess path with `agent::codex` app-server transport, reuse the event normalizer for runtime events, then add protocol fixtures and live smoke profile. |
| Full Liquid-compatible prompt engine | `SPEC.md`, `elixir/lib/symphony_elixir/prompt_builder.ex` | Partial | Initial slice uses a strict Liquid subset for common variables and `if` blocks. | Replace with a vetted Liquid crate or complete parser behind `agent::PromptRenderer`. |
| GitHub Project v2 live GraphQL adapter | `TRACKER_GITHUB_PROJECT_V2.md` | Partial | Initial adapter is dry-run/fixture capable to proceed without credentials. | Add GraphQL client, field/option cache, mutations, and credential-gated integration tests. |
| Linear live adapter | `SPEC.md`, `elixir/lib/symphony_elixir/linear/*` | Partial | Linear now has a live GraphQL adapter and fixture mode, but credential-gated smoke tests have not run in this environment and schema-sensitive mutations still need live confirmation. | Add skipped-by-default live smoke tests for reads, state update, workpad upsert, follow-up creation, and project assignment. |
| Workspace lifecycle hooks with timeout/remote SSH parity | `SPEC.md`, `elixir/lib/symphony_elixir/workspace.ex`, `SPEC.md Appendix A` | Partial | Local hooks now support timeout handling, stdout/stderr capture, `before_remove`, and safe cleanup; remote workers are deferred. | Add SSH worker trait and runtime reconciliation cleanup wiring. |
| Runtime workflow reload with last-known-good config | `SPEC.md`, `elixir/lib/symphony_elixir/workflow_store.ex` | Delayed | CLI dry-run starts from a single load. | Add file watcher/polling store and reload tests. |
| Retry timers, stall detection, and worker supervision | `SPEC.md`, `elixir/lib/symphony_elixir/orchestrator.ex` | Partial | Initial orchestrator creates deterministic dispatch plans and retry metadata only. | Add async runtime worker lifecycle, timers, continuation retry, and stall restart tests. |
| Runtime state persistence and resume wiring | `SPEC.md`, `elixir/lib/symphony_elixir/orchestrator.ex` | Partial | Tracker-neutral state model and file helpers exist, but the run loop does not yet write each transition or resume from it. | Wire runtime state into claim, workspace preparation, backend session start, event logging, handoff, and interruption recovery. |
| Token/rate-limit accounting | `elixir/docs/token_accounting.md`, `elixir/lib/symphony_elixir/orchestrator.ex` | Data model only | Needs live backend event stream. | Integrate after Codex app-server client, preserving absolute-total accounting. |
| `linear_graphql` dynamic tool | `elixir/lib/symphony_elixir/codex/dynamic_tool.ex` | Delayed | Linear adapter is not first concrete tracker. | Add backend dynamic-tool registry and Linear client implementation. |
| Operator runtime status surface | `SPEC.md`, `elixir/lib/symphony_elixir/status_dashboard.ex` | Partial | Terminal rendering now exposes polling, running/retrying/skipped categories, gate details, token counters, event-log path, and integration gaps; it is still fed by the dispatch-plan snapshot rather than a live worker runtime. | Wire the same snapshot model into the future polling runtime and reconciliation loop. |
| Optional web/API observability | `SPEC.md`, `elixir/lib/symphony_elixir_web/*` | Delayed | Terminal/status and JSONL come first. | Add HTTP layer over the runtime snapshot without coupling to orchestrator decisions. |
