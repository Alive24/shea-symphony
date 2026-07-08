# 2607 Hardening Questions

Status: Living notes

This file captures discussion before it becomes an ADR, roadmap item, or
backlog note.

## Answered

### What is the MVP baseline?

The baseline is not limited to Shea developing itself. The baseline is that the
complete workflow can run, including when Shea is used to develop another
project. Human doctor intervention is sometimes needed and acceptable in the
MVP.

### What are the runtime names?

Use `Symphony` for the hard runtime and `Shea` for the extension layer. Avoid
the term `symphony core` in user-facing milestone docs.

### Which states are standard?

The standard state set includes:

- `Backlog`
- `Todo`
- `Need to Clarify`
- `In Progress`
- `Need Human Input`
- `Agent Review`
- `Human Review`
- `Merging`
- `Rework`
- `Done`

`Agent Review` is a formal tracker state, but a Workflow Graph may disable or
bypass that stage.

`Backlog` is part of the standard state vocabulary, but it is a static tracker
queue by default. Promotion to `Todo` creates the executable condition; `Todo`
is the workflow activation point.

### Who writes tracker state?

Symphony writes tracker state. Shea and extension nodes may propose transitions
with evidence, but do not write tracker state directly.

Transition ownership should separate proposal, decision, and commit. Extensions
may influence graph direction by recommending the next edge or core node, but
Symphony validates and commits tracker state changes.

### Is GitHub Project v2 part of Symphony?

Yes. GitHub Project v2 is the first concrete tracker adapter in Symphony. The
adapter shape should leave room for Linear later.

### Are review and merge part of Symphony?

Yes. Review and merge are workflow stages, not Shea-only extension behavior.

### Is the App allowed to write?

The App is the primary operator surface. It should use Tauri backend commands to
start/query/signal/update Temporal workflows. It should not directly modify
tracker state, edit worktrees, or run workflow internals outside Temporal.

### Where should workflow graph config live?

Support both `WORKFLOW.md` and `.shea/workflow.md`, but prefer
`.shea/workflow.md`. Markdown is preferred because it can carry YAML, prose, and
templates together.

### Can standard nodes be replaced?

No. A standard node can be disabled and an extension node can be inserted in the
graph, but standard node implementation is not replaced in place.

### How are edge conditions expressed first?

Use fixed enum conditions first.

### Can extensions call LLMs?

Yes. LLM participation is allowed, but output must use a fixed schema.

### What is the workspace/config layout?

- Symphony binary is found from the local install location.
- The canonical worktree is an already cloned repository.
- Repo `.shea/` contains tracked team shared config.
- Workspaces default under `~/.shea/`, not inside the canonical worktree.
- Config precedence is workspace-local, then repo `.shea/`, then global
  `~/.shea/`.

## Still Open

### What is the first fixed LLM output schema?

Candidate fields:

- `decision`
- `evidence`
- `proposed_transition`
- `proposed_next_node`
- `questions`
- `blocked_reason`

### What is the first visual graph surface?

Current leaning: for 2607, use a state-grouped read-only workflow surface rather
than a full graph runtime. It can show Tracker State, standard behavior, current
inserted hooks/extensions, and evidence. A full graph visualizer/editor belongs
in 2608 Workflow Graph Extension or later.

### How far should Workflow Graph go in 2607?

Do not implement full lifecycle graph execution in 2607. Hardening should make
the structure clearer and move toward that direction without breaking current
workflow config.

The compatible layering is:

- Tracker State as the top-level organization layer;
- configurable standard Symphony behavior;
- insertable hooks/extensions around standard behavior;
- future graph nodes and edges derived from that structure.

Workflow Graph extension modules move to `2608 Workflow Graph Extension`.

### What is the side-effect policy for 2607?

Do not introduce a broad side-effect taxonomy yet. The hard rule is that tracker
writes go through Symphony transitions.

The first practical policy only needs to answer:

- whether workspace writes are allowed;
- whether transition requests are allowed.

External service policy should remain with existing runner/tool policy until a
real extension module system in 2608 needs stronger modeling.

### What belongs in 2608 Workflow Graph Extension?

2608 should own full Workflow Graph runtime work: graph nodes and edges as
first-class runtime objects, extension module loading, graph validation,
disabled/bypassed semantics, extension output schema, and App graph
visualization.

2607 should only prepare the shape: Tracker State grouping, configurable
standard behavior, insertion points, transition ownership, and evidence.

### What is Phase 1 allowed to do?

Phase 1 should not add user-visible features. It may add or change docs, tests,
instrumentation, internal adapters, timing, read dedupe, and small state helpers.

The no-regression baseline is:

- Main lane can run.
- Review lane can run.
- Merge lane can run.
- Doctor can report.
- App can read status.
- Existing workflows are not forced to migrate.

### What is the Phase 1 subtraction priority?

Priority areas are all important and may be investigated in parallel:

- repeated Project reads;
- scattered tracker writes;
- lane-local state mapping;
- App/read-surface source-of-truth inference;
- vendored runtime assumptions;
- CLI command shape drift;
- large files caused by mixed ownership boundaries.

Defer broad work on:

- Tauri UI structure;
- Issue Forge semantic quality;
- Dream/Reflect;
- a real plugin runtime;
- a full Workflow Graph editor.

File moves are allowed when they clarify ownership and preserve behavior. Broad
file movement without a clear boundary is deferred.

### How should runtime and tracker state conflicts be handled?

Tracker state is the external fact. Runtime state is local execution evidence.
When they conflict, Symphony should stop and reconcile rather than guessing.
If an active runtime conflict exists, move to `Need Human Input` rather than
silently continuing.

### What is the transition API shape?

Use `TrackerTransitionActivity` as the primary tracker write surface.
`IssueWorkflow` decides which transition to request. The Activity validates,
writes tracker state, writes evidence, and returns the committed transition
summary.

### What evidence is required for transitions?

Every committed transition should record issue id, from state, to state,
requester, committer, reason, workflow step id, optional future graph node id,
trace id, artifact references, and timestamp.

### How should lane handoff completion work?

Successful `TrackerTransitionActivity` completion is part of lane handoff
completion. If an Activity finishes code work but tracker transition fails,
`IssueWorkflow` must not advance as if the state changed.

### How should claim ownership work?

Use a two-layer model. Tracker fields hold coarse human-visible claim state.
Temporal workflow/activity history holds worker attempt, heartbeat, worktree,
and last progress details where possible. Local artifacts hold large details.

### Should NTC and NHI reasons be enum values?

Use enum reasons plus freeform detail. Enums support dashboard filtering and
automation; detail keeps the state useful to humans.

### How should Human Review small fixes be handled?

Before `Human Review` moves to `Merging` after a human fix, Symphony should run
lightweight validation: PR exists, branch is current enough, required checks
pass or are explicitly accepted, diff since last agent review is summarized,
human modification is acknowledged, and required review comments are resolved
or explicitly deferred.

### What is the 2607 preparation target for Workflow Graph?

Keep current workflow behavior compatible. Organize workflow structure around
Tracker State, standard behavior, insertion points, transition ownership, and
evidence. Full graph runtime and extension modules move to 2608.

### What is special about Human Review?

`Human Review` may transition to `Rework`, or a human may make a small fix and
then approve the issue into `Merging`.

### What feels slow first?

App refresh is the clearest subjective pain point. More broadly, the system
often feels slow after LLM work has already completed, which suggests repeated
control-plane work rather than LLM latency.

### What should App refresh read?

The top-level dashboard should start from a SQLite-backed
`dashboard_snapshot` materialized by Symphony runtime paths. It should show
current operational lane items, human todo items, concise PR number/status,
local Temporal/worker availability, and workflow state needed for display.

The dashboard should not show worktree path, branch name, full traces, or full
artifact bodies. Those belong in lane item detail and should be loaded lazily
after drill-down.

Temporal Query remains the preferred source for one issue's authoritative
runtime state. SQLite is for aggregate dashboard reads, tracker cache, PR
summary cache, artifact index, and freshness markers.

### Does SQLite replace Temporal Query?

No. Temporal Query reads deterministic per-workflow state from Temporal history
and worker replay/cache. It is the right tool for one issue's current workflow
state.

SQLite is the local read model/cache/index for data that is awkward or slow to
compute by querying many workflows, tracker items, and artifact directories on
each render. It is durable and useful for future async sync/export, but remains
rebuildable and non-authoritative for workflow progression.

### Who can write SQLite?

SQLite writes happen through Symphony runtime/backend boundaries, not UI
components. The App may trigger commands, but UI components do not mutate the
DB directly.

SQLite may accelerate reads, but it must not authorize workflow progression or
tracker transitions. Workflow decisions depend on Temporal state, Activity
results, and targeted tracker readback.

### How should SQLite projection work in 2607?

Use synchronous projection first. Activity and backend command results update
SQLite directly through a small projection/store boundary. Do not add an
independent async projector loop in 2607.

Keep the interface shaped so later async replay, export, or cloud sync can be
added without changing workflow semantics.

### How fresh must dashboard data be?

Dashboard data is eventually consistent. Ordinary render reads SQLite and
shows freshness/staleness. Explicit refresh paths may run tracker, Temporal, or
artifact refresh work through Symphony runtime boundaries.

Selected issue detail can use Temporal Query for authoritative workflow state.

### When is SQLite rebuilt?

Do not full-rebuild SQLite on every startup. Startup performs lightweight
schema and health checks. Rebuild happens when the DB is missing, corrupt,
schema-incompatible, explicitly requested, or required by a recovery path.

### Should SQLite use an ORM?

No. SQLite is a local read model/cache/index, not the business fact layer or
domain model. 2607 should use a typed DB access layer rather than
ActiveRecord-style object loading/saving.

Use `LocalStateReader`, `LocalStateProjector`, and `LocalStateAdmin` as the
typed boundaries. Prefer `rusqlite`, handwritten SQL, typed DTOs, and minimal
repository functions. Symphony runtime owns schema versioning and minimal SQL
migrations; do not introduce a heavy ORM or external migration service in 2607.

### What is the first SQLite schema?

Use five initial tables:

- `workflow_index`;
- `tracker_cache`;
- `artifact_index`;
- `activity_progress`;
- `meta`.

Do not add a `dashboard_snapshot` blob table first. Assemble dashboard reads
from the indexed tables. Do not add a dedicated `tracker_mutation_log` table
first; Temporal history is the durable mutation ledger and SQLite projects
current observability through `activity_progress`, `tracker_cache`, and
artifact refs.

### How should PR-to-issue linking be made reliable?

Treat PR linking as a durable idempotent tracker mutation, not an incidental
post-PR command.

Use a stable mutation key such as
`link-pr:<repo-id>:<issue-number>:<pr-number>`. The Activity must write and
then read back. Success means the desired PR relation is confirmed by tracker
readback.

If PR creation succeeds but linking is not confirmed, `IssueWorkflow` should
retry the mutation according to typed policy. It should not move to the next
handoff state as if the PR relation succeeded. If retry is exhausted or blocked,
move to `Need Human Input` with evidence.

SQLite does not become the mutation ledger. It only projects observable current
or recent state for the App.

Project history and broad queue browsing should stay in the tracker.

### How should App-triggered commands be bounded?

The App should use Tauri backend commands for dashboard reads, issue detail
reads, workflow start, refresh/cache requests, and local DB health/rebuild.

Human input, approval, human fix, and request-rework actions should be routed
to Codex/operator flows. The routed flow calls the Symphony/Temporal interface
with structured results; the App does not implement those semantics directly.

Automatic doctor checks should be Temporal Activities. Human doctor work should
open a Codex/operator flow that reports back through Temporal.

Opening worktrees and resuming agents should not be App imperatives. They should
be workflow/activity outcomes or artifact links.

For refresh/cache requests, prefer Temporal Update when the UI needs
accepted/rejected feedback. The refresh itself can proceed asynchronously and
surface progress through workflow state plus SQLite projection.

### How do routed Coding Agent flows submit actions?

Use an Operator Action Bridge tool/MCP interface as the primary path, not CLI.

The App asks Symphony to prepare an `OperatorActionContext`, then opens the
Codex/operator flow with a context path and brief. The routed flow calls a
narrow tool such as `submit_operator_action(context_id, action, payload)`.

The bridge validates context existence, expiry, allowed action, and payload
schema, then submits a Temporal Update. Workflow validation runs again before
any Activity changes tracker/read-model state.

Coding Agent flows should not receive raw tracker mutation access, raw Temporal
client access, or SQLite write access. CLI wrappers, if any, are debug/admin
fallback only.

### What are the first concrete performance targets?

Current leaning: start with relative targets before hard numbers:

- SQLite-backed dashboard refresh;
- Temporal Query-backed issue detail refresh;
- no mutating command from UI refresh;
- non-LLM path should be seconds-scale unless waiting on external services.

### What is the 2607 Temporal decision?

Temporal is the 2607 runtime spine, not a future spike. `IssueWorkflow`
understands all standard Shea Symphony states from the start, including
`Backlog`, but Workflow executions are activation episodes for executable
states rather than long-lived executions for every issue. The old
autopilot/tick/resume loop is legacy-to-delete. Temporal Cloud is out of scope;
the runtime is local.

### What is the Temporal concurrency model?

Use at most one active `IssueWorkflow` execution per issue at a time. Tracker
is the durable queue and external state between workflow activations.
`IssueWorkflow` is an executable orchestration episode, not a live execution
for every issue from `Backlog` to `Done`.

A single Workflow execution is a deterministic ordered state machine; it does
not mutate Workflow state concurrently. Signals and Updates are processed in
workflow-history order, but Workflow code still validates tracker state and
allowed actions.

Parallelism happens through multiple active Workflow episodes, Activities,
Child Workflows when needed, and Worker pools. Multiple executable issues can
run concurrently. Within one active issue episode, read-only or non-conflicting
Activities may run concurrently.

External fact-changing operations remain serial, idempotent, and
readback-verified: tracker transitions, PR-to-issue linking, merge/land,
terminal writes, and claim cleanup.

Codex/Main/Review/Merge stay coarse Activity boundaries, not per-model-turn
Activities. SQLite projection may be concurrent at request level, but DB writes
use short retryable transactions and remain non-authoritative.

Design implication: Temporal is the durable concurrency spine for active work,
and tracker is the durable queue between activations. Shea should not rebuild
an autopilot scheduler. Active `IssueWorkflow` episodes own ordered per-issue
decisions; worker pools own parallel external work.

### What activates an IssueWorkflow?

Workflow population is based on executable tracker states, not every
Shea-managed issue.

Static tracker lanes do not start a live Workflow execution by default:

- `Backlog`;
- `Human Review`;
- `Need Human Input`, unless automatic doctor/reconcile work is required;
- `Done`.

Executable states can start or resume a workflow episode:

- `Todo`;
- `In Progress`;
- `Agent Review`;
- `Rework`;
- `Merging`;
- `Need Human Input` only for automatic doctor/reconcile work.

Backlog promotion to `Todo` creates the executable condition. Human Review
approval, request-rework, or human-fix actions arrive through the Operator
Action Bridge and then start the appropriate validation, rework, or merging
episode.

The Symphony reconciler/App start command reads tracker states and starts
episodes only for executable states, up to configured capacity.

### What is the starting task queue topology?

Do not start with a single task queue in 2607. Single queue is too coarse for
Shea because long-running Coding Agent Activities can delay short
control-plane work.

Start with:

- `symphony-core`: `IssueWorkflow`, tracker transitions, PR-to-issue link
  mutation, workflow control Activities, latency-sensitive low/medium
  concurrency work;
- `symphony-agent`: Codex Main/Rework/Merge runs and heavy Agent Review backend
  work; long-running, resource-heavy, very low concurrency;
- `symphony-local`: SQLite projection, artifact indexing, tracker cache
  refresh, local health/admin/rebuild; short local work with higher
  concurrency.

Use Activity-level concurrency limits within each queue. Do not split into many
more queues until measurement proves it.

Initial defaults are configurable:

- `symphony-core`: up to 3 concurrent control-plane Activities, but
  fact-changing writes remain serial per issue;
- `symphony-agent`: up to 3 concurrent agent runs, matching prior Codex
  app-server/agy CLI dogfood capacity, but only one active agent attempt per
  issue;
- `symphony-local`: start higher, around 8 local Activities, with short
  retryable SQLite write transactions.

Design reason: task queues are for worker capacity, isolation, and routing.
The goal is to keep long-running Coding Agent Activities from delaying tracker
transitions, PR link verification, refresh/cache projection, and other
control-plane work.

### How should Merging handle semantic fixes?

`Merging` may perform semantic fixes in place through `MergeActivity` or a
dedicated `MergeSemanticFixActivity`. If it cannot resolve the problem, move to
`Need Human Input`, not `Rework`.

### Does 2607 need an independent local Symphony service?

No. The Tauri backend command layer should call Temporal client APIs directly.
Do not design an independent local Symphony service in 2607.

### What is CLI for after Temporal?

CLI is admin/dev fallback only: initialize local config when App is unavailable,
run local doctor/self-checks, run the worker for development or CI, and expose
thin debug wrappers. CLI does not own workflow product operations.

Product commands such as autopilot, main loop, review loop, merge loop, and
mutating doctor should become Temporal Signals, Queries, Updates, Activities,
or compatibility shims while being removed from the CLI product surface.

### What is the Activity grain for coding agents?

Use coarse attempt-level Activities. Codex app-server should be allowed to run
its own internal agent/tool loop and evolve independently. Temporal should
track the durable attempt boundary, heartbeat/progress, result summary, and
artifact references, not every model turn or tool call.

The same rule applies to future coding agents.

### Should Agent Review be named after Gemini?

No. Use `AgentReviewActivity` as the core Activity name. Gemini may remain the
configured backend, but the workflow state machine should not be coupled to a
provider name.

### How should Human Todo be presented?

The App can aggregate `Need Human Input` and `Human Review` into one human todo
surface. Detail views must keep the underlying state explicit because `Need
Human Input` is a mid-workflow unblock state while `Human Review` is the formal
approval gate.

### What should the core runtime package be named?

Use `symphony`. Temporal is the runtime spine and default context inside
Symphony, so a separate `temporal_runtime` package name is unnecessary unless
implementation constraints force it. Shea extension code can still be
Temporal-backed; the boundary is ownership, not Temporal usage.

### Should tracker transition code wrap the old lane mutation model?

No. `TrackerTransitionActivity` should become the new owner. It should migrate
existing tracker adapter, recovery marker, readback, workpad, and audit
behavior into the Temporal Activity boundary. Do not preserve the old
autopilot/lane mutation model as a target wrapper.

### Should transition Activity payloads use full `TrackerIssue`?

No. Use small request/result DTOs in Workflow history. This gives up having the
complete issue description, workpad, comments, project fields, linked PR
payloads, and rich evidence directly in Temporal history. That loss is
intentional: rich details belong in artifacts, tracker comments/workpads, or
targeted Activity reads.

### How should Project field bloat be handled?

Reduce it. GitHub Project fields should keep human-visible workflow state,
coarse ownership, PR/status facts, and terminal/blocker facts. Detailed local
runtime state belongs in Temporal workflow state, Activity progress, and local
artifacts. If one issue's local state becomes unrecoverable, stop that issue,
clear local state, and rebuild from tracker state plus durable artifacts.

### Should tracker transition migration be a tiny first batch?

No. Use submilestones for reviewability, but the 2607 hardening target is
complete tracker transition ownership before new feature development resumes.
Do not leave unknown direct tracker write paths as acceptable final scope.

### What should `IssueWorkflow` store durably?

Store small resumable control state: workflow id, repo id, issue ref, tracker
backend, last committed tracker state, active step, attempts, structured
waiting state, last transition summary, run summaries, artifact refs, PR
summary, human todo summary, and runtime health summary.

Do not store full issue descriptions, workpads, comments, diffs, transcripts,
review reports, full Project field dumps, or full worktree status. Those belong
in tracker reads, artifacts, or issue detail lazy-load paths.

### How should App queries be layered?

Use `dashboard_snapshot` for lightweight operational summaries and
`issue_detail_snapshot` for one issue's attempt summaries, waiting detail,
recent artifact refs, review verdict summary, and merge summary. Both return
artifact refs, not artifact bodies.

### How should Activity failures be classified?

Use shared outcome classes: `success`, `already_applied`, `retryable`,
`wait_and_retry`, `need_human_input`, `conflict`, `rejected`,
`terminal_noop`, and `unhandled_error`.

Activities report typed outcomes. `IssueWorkflow` decides retry, wait,
reconcile, or state transition. Existing tracker error kinds, review backend
recovery policies, Codex/app-server statuses, merge repair retryability, and
doctor findings should map into this taxonomy rather than becoming separate
state machines.

### How should land skill flow be represented?

Current leaning: `Merging` uses a configured land runner by default and does not
call `gh pr merge` directly from arbitrary extension logic.
