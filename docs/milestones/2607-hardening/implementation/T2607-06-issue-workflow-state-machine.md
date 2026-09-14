# T2607-06 IssueWorkflow State Machine

Design Status: Draft

## Purpose

Implement `IssueWorkflow` as the durable owner of one executable orchestration
pulse.

The Workflow starts from an executable tracker state, runs one or more
executable lane handlers, commits tracker transitions through Activities, and
then completes at a real boundary: static handoff, `Done`, cancellation, or
unhandled error.

## Inputs

This package implements decisions from:

- `ISSUE-WORKFLOW.md`;
- `ISSUE-WORKFLOW-STATE.md`;
- `WORKFLOW-ACTIVATION.md`;
- `ACTIVITY-ERROR-TAXONOMY.md`;
- `T2607-04-tracker-transition-activity.md`;
- `AGENT-ACTIVITY-CONTRACT.md`;
- `OPERATOR-ACTION-BRIDGE.md`.

## Goals

- Implement `IssueWorkflowInput` and durable `IssueWorkflowState`.
- Support all standard Shea Symphony tracker states in the state vocabulary.
- Start from any executable state.
- Keep static lanes out of live idle Workflow execution by default.
- Make executable lane handlers independently startable and internally
  chainable.
- Use terminal outcomes only for real execution boundaries.
- Route all tracker transitions through `TrackerTransitionActivity`.
- Route coding/review/merge/doctor work through coarse Activities.
- Provide Temporal Query responses for one issue's current runtime detail.
- Validate Signals/Updates against current state and allowed actions.

## Non-Goals

- No live Workflow execution for every tracker issue.
- No idle Workflow for `Backlog`, `Human Review`, or normal
  `Need Human Input`.
- No custom autopilot scheduler inside Workflow code.
- No direct tracker writes from Workflow code.
- No direct filesystem, tracker, artifact, or SQLite I/O from Query handlers.
- No large transcripts, diffs, reports, or workpads in durable Workflow state.
- No terminal outcome for internal chaining between executable handlers.

## Expected Code Areas

Recommended package shape:

```text
symphony/
  workflow/
    issue_workflow.rs
    input.rs
    state.rs
    query.rs
    update.rs
    handlers/
      todo.rs
      in_progress.rs
      agent_review.rs
      rework.rs
      merging.rs
      need_human_input.rs
    routing.rs
    activity_results.rs
```

Names are illustrative. Keep deterministic Workflow code separate from
Activity implementations and adapter I/O.

## Workflow Input

Recommended input:

```text
IssueWorkflowInput {
  workflow_id
  repo_id
  issue_ref
  from_tracker_state
  target_kind
  source_ref
  source_tracker_revision
  started_at
  operator_action_ref?
  capacity_policy_ref?
}
```

Rules:

- `from_tracker_state` must be executable for a normal start.
- `source_ref` explains why the pulse started.
- The input records the observed tracker fact at start.
- The Workflow should not infer its purpose by broad tracker scanning.
- Targeted tracker validation happens through Activities at durable
  boundaries.

## Durable State DTO

Implement a small, versionable state object:

```text
IssueWorkflowState {
  workflow_id
  run_id?
  repo_id
  issue_ref
  tracker_backend
  from_tracker_state
  target_kind
  source_ref
  source_tracker_revision
  started_at
  operator_action_ref?
  capacity_policy_ref?
  current_tracker_state
  active_step
  active_attempt?
  terminal_outcome?
  waiting?
  last_transition?
  active_agent_run?
  active_review_run?
  active_merge_run?
  artifact_refs
  pr_summary?
  human_todo_summary?
  runtime_health_summary?
}
```

Store summaries and refs, not bodies. Add fields additively when possible.

## State Vocabulary

The Workflow state vocabulary must include:

- `Backlog`;
- `Todo`;
- `Need to Clarify`;
- `In Progress`;
- `Need Human Input`;
- `Agent Review`;
- `Human Review`;
- `Rework`;
- `Merging`;
- `Done`.

Not every state is an activation state.

Executable starts:

- `Todo`;
- `In Progress`;
- `Agent Review`;
- `Rework`;
- `Merging`.

Static waits:

- `Backlog`;
- `Need to Clarify`;
- normal `Need Human Input`;
- `Human Review`;
- `Done`.

Doctor/reconciliation or a routed operator action must establish an executable
tracker state before Coordinator starts a new episode; `Need Human Input` is
never itself an activation state.

## Handler Model

Each executable state has a handler:

```text
handle_todo(state) -> HandlerResult
handle_in_progress(state) -> HandlerResult
handle_agent_review(state) -> HandlerResult
handle_rework(state) -> HandlerResult
handle_merging(state) -> HandlerResult
handle_need_human_input_auto(state) -> HandlerResult
```

Recommended handler result:

```text
HandlerResult {
  next_state
  evidence_refs
  artifact_refs
  transition_reason
  waiting?
  terminal?
}
```

If `next_state` is executable, the Workflow may continue to the next handler in
the same execution. This is an internal continuation, not a terminal outcome.

If `next_state` is static or `Done`, the Workflow commits the tracker
transition and completes.

## Terminal Outcomes

Only expose these terminal outcomes:

- `completed_static_handoff`;
- `completed_done`;
- `failed_unhandled_error`;
- `cancelled`.

Do not expose a terminal outcome for “completed executable chain.” If one
handler moves into another executable state and the Workflow continues, the
pulse has not externally completed.

Terminal outcome rules:

- `completed_static_handoff`: tracker was committed to a static wait lane such
  as `Human Review`, `Need to Clarify`, normal `Need Human Input`, or
  `Backlog`;
- `completed_done`: tracker was committed to `Done`;
- `failed_unhandled_error`: Workflow cannot safely converge or hit an internal
  invariant failure;
- `cancelled`: operator/system cancellation policy ended the execution.

## Tracker Transition Calls

All tracker transitions use `TrackerTransitionActivity`.

Workflow transition request construction must include:

- `workflow_id`;
- `issue_ref`;
- `expected_from_state`;
- `requested_to_state`;
- transition kind;
- reason enum and optional detail;
- evidence refs;
- idempotency key.

When transition returns:

- `committed`: update `current_tracker_state`, `last_transition`, and continue;
- `already_applied`: update state from observed target and continue;
- `retry_later`: wait or schedule retry according to policy;
- `conflict`: route according to conflict policy, usually
  `Need Human Input`;
- `need_human_input`: commit or remain in structured wait;
- `unhandled_error`: fail with `failed_unhandled_error`.

Workflow code must not force-write around `expected_from_state` conflicts.

A handler has not completed its handoff until all policy-required tracker
mutations and readbacks succeed. Local agent completion, a pushed branch, or a
successful write call is evidence of progress, not workflow completion. If a
commit cannot be confirmed, retain the execution evidence and route to retry,
reconciliation, or a structured human wait.

## Activity Routing

Activity outcomes are normalized before Workflow routing.

General routing:

- `success`: continue;
- `already_applied`: continue when idempotent;
- `retryable`: let Temporal retry or route after retry exhaustion;
- `wait_and_retry`: use durable timer or retry-after policy;
- `need_human_input`: create `WaitingState` and transition to
  `Need Human Input`;
- `conflict`: reconcile or transition to `Need Human Input`;
- `rejected`: follow normal graph edge such as `Todo -> Need to Clarify` or
  `Agent Review -> Rework`;
- `terminal_noop`: complete or transition according to terminal policy;
- `unhandled_error`: fail with `failed_unhandled_error`.

The Workflow owns routing. Activities report facts and typed outcomes.

## Todo Handler

Purpose:

- confirm issue contract/readiness before implementation.

Activities:

- `ContractCheckActivity`;
- `TrackerTransitionActivity`.

Possible outcomes:

- contract ready: transition to `In Progress` and continue;
- contract insufficient: transition to `Need to Clarify` and complete with
  `completed_static_handoff`;
- operational blocker: transition to `Need Human Input` and complete with
  `completed_static_handoff`;
- malformed internal state: `failed_unhandled_error`.

Backlog promotion may create the `Todo` tracker state. The readiness check that
decides whether work can start belongs in this executable handler.

## In Progress Handler

Purpose:

- run Main implementation work.

Activities:

- worktree lease acquire/reuse;
- `MainAgentActivity`;
- PR-to-issue link mutation through `TrackerTransitionActivity`;
- artifact write/index projection;
- tracker transition.

Possible outcomes:

- implementation with required PR/link confirmed: transition to
  `Agent Review` and continue;
- implementation does not require PR by policy: transition according to
  configured next state;
- missing secret, permission, external blocker, unsafe operation, or link
  failure after retry exhaustion: transition to `Need Human Input` and
  complete;
- invalid agent result: `failed_unhandled_error`.

Implementation handoff is not complete until tracker state transition and any
required PR relation readback have succeeded.

## Agent Review Handler

Purpose:

- agentic review gate before human attention.

Activities:

- `AgentReviewActivity`;
- optional safe-autofix inside the review Activity according to capability;
- tracker transition.

Verdict routing:

- `pass`: transition to `Human Review` and complete;
- `pass_with_comments`: usually transition to `Human Review` with comments as
  evidence, unless policy routes to rework;
- `safe_autofix_applied`: validate configured evidence, then route to
  `Human Review` or continue review policy;
- `request_rework`: transition to `Rework` and continue;
- `need_human_input`: transition to `Need Human Input` and complete;
- `unhandled_error`: fail with `failed_unhandled_error`.

Agent Review may be configurable, but it is a standard state in 2607.

## Rework Handler

Purpose:

- run implementation changes from review or human feedback.

Activities:

- worktree lease acquire/reuse;
- `ReworkActivity`;
- artifact write/index projection;
- tracker transition.

Possible outcomes:

- feedback addressed: transition to `Agent Review` and continue;
- feedback pushed back but evidence is valid: route according to policy,
  usually `Agent Review` or `Need Human Input`;
- unresolved blocker: transition to `Need Human Input` and complete;
- invalid result: `failed_unhandled_error`.

Merge-time semantic fix failure should not route here by default.

## Merging Handler

Purpose:

- land approved work and finish terminal tracker state.

Activities:

- `HumanReviewValidationActivity` when started from approval/human fix context;
- `MergeActivity`;
- optional semantic fix inside merge Activity or dedicated merge-fix boundary;
- tracker transition to `Done`;
- artifact write/index projection.

Possible outcomes:

- merge/readback succeeds: transition to `Done` and complete with
  `completed_done`;
- already merged and terminal facts read back: transition or confirm `Done`;
- pending checks/rate limits: wait and retry according to policy;
- semantic fix cannot resolve safely: transition to `Need Human Input` and
  complete;
- permission, dirty unsafe worktree, unknown mergeability, or external service
  failure: transition to `Need Human Input` and complete;
- malformed internal state: `failed_unhandled_error`.

Do not bounce merge-time semantic fix failure to `Rework` by default.

## Need-State Reasons

Static waits use a closed reason code plus optional human-readable detail.
Initial `Need to Clarify` reasons are:

- `missing_contract`;
- `ambiguous_scope`;
- `insufficient_acceptance_criteria`;
- `conflicting_requirements`.

Initial `Need Human Input` reasons are:

- `requires_secret`;
- `requires_external_account`;
- `dangerous_operation`;
- `external_service_failure`;
- `manual_decision_required`;
- `local_environment_blocked`;
- `tracker_state_conflict`;
- `merge_semantic_fix_failed`.

The enum supports routing and filtering. Detail explains the concrete blocker
without expanding the state vocabulary.

## Human Review To Merging

An operator may request `Rework`, approve the reviewed result, or make a small
fix before requesting `Merging`. Before merge work begins,
`HumanReviewValidationActivity` verifies the current external facts:

- the PR still exists;
- branch freshness satisfies repository policy;
- required checks pass or have an explicit operator acceptance;
- changes since the last Agent Review are summarized;
- human-authored modifications are acknowledged;
- required review comments are resolved or explicitly deferred.

Materially risky changes route back to `Agent Review` or `Rework`. A semantic
fix that the merge boundary cannot complete safely routes to `Need Human Input`
with reason `merge_semantic_fix_failed`, not to `Rework` by default.

## Need Human Input Resumption

Purpose:

- define how a Doctor/reconciliation or routed operator action may resolve a
  static human wait before a later executable activation.

Inputs:

- existing `WaitingState`;
- operator action ref, if routed;
- doctor/reconcile reason, if automatic.

Possible paths:

- routed human input resolves blocker: transition to resume target and continue
  if executable;
- routed request-rework action: transition to `Rework` and continue;
- routed cancellation/no-op: transition according to terminal policy;
- automatic Doctor safe repair succeeds: commit an executable resume state,
  then allow Coordinator to evaluate that new tracker observation;
- automatic doctor cannot repair safely: remain or transition to
  `Need Human Input` with updated evidence and complete.

The App does not directly implement this policy. It routes operator work
through the Operator Action Bridge, and the Workflow validates submitted
actions.

## Signal And Update Handling

Use Updates for state-changing operator actions that need synchronous
accepted/rejected feedback:

- `submit_human_input`;
- `approve_human_review`;
- `request_rework`;
- `submit_human_fix`;
- `doctor_handoff_result`;
- `cancel_issue_workflow`.

Validation rules:

- workflow is active and in a state that accepts the action;
- action matches an unexpired `OperatorActionContext`;
- payload schema is valid;
- evidence refs exist or are explicitly unavailable;
- duplicate submission policy is honored;
- target transition is allowed from current state.

Signals may be used for low-risk supplemental notes or evidence refs.

## Query Surfaces

Implement an issue-detail Query over durable Workflow state:

```text
query_issue_detail() -> IssueDetailSnapshot
```

Query may return:

- current tracker state last confirmed through `TrackerTransitionActivity`;
- active step;
- waiting detail;
- attempt summaries;
- PR summary;
- recent artifact refs;
- last transition;
- runtime health summary;
- terminal outcome.

Query must not perform:

- filesystem I/O;
- tracker I/O;
- SQLite reads;
- artifact body reads;
- network calls.

Dashboard-wide reads should use SQLite local state DB, not fan out through
every active and historical Workflow Query.

## Determinism Rules

Workflow code must be deterministic:

- no direct system time calls outside Temporal APIs;
- no filesystem or network I/O;
- no random IDs outside deterministic generation or Activity results;
- no direct process spawning;
- no reading local config during replay;
- no hidden tracker reads.

Side effects happen in Activities.

## SQLite Projection

Workflow should not write SQLite directly.

Projection happens through Activities or backend boundaries after Workflow or
Activity state changes:

- workflow start/running/terminal summaries;
- tracker transition summaries;
- activity progress summaries;
- artifact refs;
- waiting/human todo summaries.

Projection failure must not alter Workflow truth. It marks local read-model
freshness stale or failed.

## Migration Steps

### IWSM-1: Input And Durable State

- Define `IssueWorkflowInput`.
- Define versionable `IssueWorkflowState`.
- Define terminal outcome enum.
- Define waiting, transition, attempt, artifact, PR, and health summaries.

### IWSM-2: Handler Skeletons

- Implement handler dispatch from executable start state.
- Implement handler result routing.
- Implement internal chaining without exposing a terminal outcome.
- Reject non-executable starts with typed failure.

### IWSM-3: Activity Integration

- Wire `ContractCheckActivity`.
- Wire Agent Activities.
- Wire `TrackerTransitionActivity`.
- Wire artifact/local projection boundaries.
- Normalize Activity outcomes into Workflow routing.

### IWSM-4: Human And Operator Actions

- Implement Update validation.
- Implement Operator Action Bridge submission handling.
- Implement cancellation behavior.
- Implement Need Human Input resume routing.

### IWSM-5: Query And Projection

- Implement issue-detail Query.
- Project small summaries to SQLite through local projection boundaries.
- Verify Query handlers perform no I/O.

### IWSM-6: Delete Old Loop Routing

- Remove old lane-local completion routing after equivalent Workflow handlers
  exist.
- Ensure old autopilot/tick/resume does not remain a second state machine.

## Acceptance Checks

- Workflow can start from every executable state.
- Non-executable static starts are rejected or ignored before Workflow start by
  Coordinator.
- Each executable handler is independently startable.
- Executable handlers can chain internally without terminal completion.
- Terminal outcomes are only `completed_static_handoff`, `completed_done`,
  `failed_unhandled_error`, and `cancelled`.
- Tracker transitions use `TrackerTransitionActivity` with preconditions.
- Agent work uses coarse Agent Activities.
- Need Human Input waits are structured and resumable.
- Operator Updates are validated against current state and context.
- Issue-detail Query returns useful summaries without I/O.
- Large evidence remains in artifacts, not Workflow state.

## Done Means

- `IssueWorkflow` owns executable pulse orchestration;
- all standard states are represented;
- executable handlers are implemented and chainable;
- static lane handoff completes the Workflow;
- `Done` terminal completion is explicit;
- activity outcome routing is centralized;
- issue-detail Query works without external I/O;
- old lane loop routing is deleted or blocked as a second workflow engine.
