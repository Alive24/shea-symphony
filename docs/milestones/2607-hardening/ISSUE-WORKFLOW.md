# IssueWorkflow

Status: Draft

## Purpose

Define the Temporal `IssueWorkflow` state flow for Shea Symphony.

`IssueWorkflow` should understand all standard Shea Symphony tracker states,
including static lanes such as `Backlog` and `Human Review`. It should not
stay open from `Backlog` through `Done` for every issue.

Durable Workflow state is defined in `ISSUE-WORKFLOW-STATE.md`. The Workflow
stores resumable control state, summaries, and artifact refs; rich issue
payloads, transcripts, diffs, review reports, and artifact bodies stay outside
Workflow state.

Use at most one active `IssueWorkflow` execution per issue at a time. Tracker
is the durable queue between workflow activations. An `IssueWorkflow` execution
is an executable orchestration episode, not a live idle workflow for every
Shea-managed issue.

Activity failure routing is defined in `ACTIVITY-ERROR-TAXONOMY.md`. Activities
report typed outcomes; `IssueWorkflow` decides retry, wait, conflict handling,
or state transition.

## Workflow Input

Every `IssueWorkflow` execution starts from an executable tracker state.

Recommended input:

```text
IssueWorkflowInput {
  workflow_id
  repo_id
  tracker_backend
  issue_ref
  from_tracker_state
  target_kind
  source_kind
  source_ref
  source_tracker_revision
  started_at
  audit_reason
  operator_action_ref?
  capacity_policy_ref?
}
```

The input tells the Workflow why this execution started and which tracker fact
it observed. The Workflow should not infer its starting purpose by scanning
tracker state again after start.

`started_at` retains its durable wire name and records the pre-start activation
episode timestamp in RFC 3339 UTC second precision. It is not Temporal's
authoritative execution start time; Coordinator Describe observations call
that separate value `temporal_started_at`. New fields use Serde defaults so
histories containing the earlier input shape remain replay-compatible, while
new Coordinator construction always populates them.

## Standard States

- `Backlog`
- `Todo`
- `Need to Clarify`
- `In Progress`
- `Need Human Input`
- `Agent Review`
- `Human Review`
- `Rework`
- `Merging`
- `Done`

## State Flow

```text
Backlog
  -> Todo
  -> Need to Clarify
  -> In Progress
  -> Need Human Input
  -> Agent Review
  -> Human Review
  -> Rework
  -> Merging
  -> Done
```

This diagram shows the full state vocabulary, not every allowed edge.

## Execution Model

Executable lane handlers are independently startable and internally chainable.

Coordinator may start an `IssueWorkflow` execution from any executable tracker
state: `Todo`, `In Progress`, `Agent Review`, `Rework`, or `Merging`.
`Need Human Input` remains static; Doctor/reconciliation must first establish
and commit an executable tracker state before Coordinator evaluates activation.

Workflow start selects the handler from `from_tracker_state`. When a handler
finishes:

- if the next state is executable, the Workflow may continue to the next
  handler in the same execution;
- if the next state is static, the Workflow commits the tracker transition and
  completes;
- if the next state is `Done`, the Workflow commits terminal state and
  completes;
- if it cannot safely decide or act, it commits `Need Human Input` or fails
  with `failed_unhandled_error` according to the error taxonomy.

Chaining executable handlers is an internal continuation. Terminal outcomes
exist only at static handoff, `Done`, unhandled error, or cancellation.

Terminal Workflow outcomes:

- `completed_static_handoff`;
- `completed_done`;
- `failed_unhandled_error`;
- `cancelled`.

## Tracker Transition Boundary

Every tracker state transition uses `TrackerTransitionActivity`.

Transition requests must carry:

- `expected_from_state`;
- `requested_to_state`;
- `reason_enum` and optional `reason_detail`;
- `evidence_refs`;
- `workflow_id`;
- source revision when the tracker backend provides one.

If the tracker state no longer matches `expected_from_state`, the Activity
returns a typed conflict. Workflow should not force-write around the
precondition.

## Backlog

Purpose:

- candidate issue intake;
- explicit promotion into `Todo`.

Entry:

- issue enters Shea-managed project backlog;
- issue is captured by Issue Forge, Dream/Reflect, or manual operator intake.

Signals or updates:

- `promote_to_todo`;
- `reject_or_archive_backlog_item`;
- `submit_backlog_context`.

Exit:

- to `Todo` when promoted and quality gate passes;
- remain in `Backlog` when more backlog shaping is needed.

Notes:

- Do not start implementation from `Backlog`.
- `Backlog` is a static tracker queue by default. It does not keep a live
  `IssueWorkflow` execution open.
- Promotion to `Todo` creates the executable condition. `Todo` is the workflow
  activation point for contract check and implementation entry.

## Todo

Purpose:

- confirm the issue contract is ready for implementation.

Activities:

- `ContractCheckActivity`.

Exit:

- to `Need to Clarify` when the contract is insufficient;
- to `In Progress` when ready.

## Need To Clarify

Purpose:

- wait for human clarification before implementation starts.

Signals or updates:

- `submit_clarification`;
- `cancel_issue_workflow`.

Exit:

- to `Todo` after clarification so `ContractCheckActivity` can run again;
- to `Need Human Input` if clarification exposes an operational blocker.

## In Progress

Purpose:

- Main agent implementation.

Activities:

- `MainAgentActivity`;
- `TrackerTransitionActivity` mutation operation for PR-to-issue linking when a
  PR is created;
- `ArtifactWriteActivity`;
- `TrackerTransitionActivity`.

`MainAgentActivity` follows `AGENT-ACTIVITY-CONTRACT.md`: attempt-level
execution, `code_write` capability, worktree lease, layered heartbeat, typed
result, and artifact refs for large evidence.

Exit:

- to `Agent Review` on implementation completion after PR existence and
  PR-to-issue relation are confirmed when a PR is required;
- to `Need Human Input` on secret, permission, dangerous operation, external
  failure, tracker conflict, or other human-needed blocker.

Handoff requirements:

- PR exists when the implementation path requires a PR;
- PR-to-issue relation is confirmed by tracker readback;
- transition to the next tracker state is committed;
- local read model projection is updated or explicitly marked stale.

If PR creation succeeds but PR-to-issue linking is not confirmed, the Workflow
must retry the durable mutation according to policy. It should not advance as
if implementation handoff is complete. If retry policy is exhausted or blocked,
move to `Need Human Input` with concrete evidence.

## Need Human Input

Purpose:

- wait for human/operator input after an active workflow hits a blocker.

Required workflow data:

- `reason`;
- `resume_target`;
- `blocking_artifact_refs`;
- `recommended_next_action`.

In durable state this should be represented as a structured `WaitingState`, so
the App can aggregate Human Todo items without losing whether this is
`Need Human Input`, `Need to Clarify`, or `Human Review`.

Signals or updates, normally submitted by a routed Coding Agent/operator flow
rather than App UI code:

- `submit_human_input`;
- `cancel_issue_workflow`;
- `request_rework`.

Exit:

- to `resume_target` when the blocker is resolved;
- to `Rework` if the human input changes the implementation contract;
- to `Done` only for explicit cancellation or terminal no-op policy.

Notes:

- `Need Human Input` is a static tracker lane after the workflow
  records the blocker and evidence.
- Start an `IssueWorkflow` execution or find an existing active execution only
  after a routed operator or Doctor/reconciliation action has established an
  executable tracker state.

## Agent Review

Purpose:

- agentic review gate before spending human attention.

Activities:

- `AgentReviewActivity`;
- `TrackerTransitionActivity`;
- `ArtifactWriteActivity`.

`AgentReviewActivity` follows `AGENT-ACTIVITY-CONTRACT.md`. It may be
configured as `review_read_only`, `review_comment`, or `review_safe_autofix`.

Exit:

- to `Human Review` when review passes;
- to `Rework` when review finds actionable implementation issues;
- to `Need Human Input` when review needs human judgment.

Notes:

- This is a standard state.
- It may be configurable in future workflow structure, but 2607 should model it
  explicitly.

## Human Review

Purpose:

- formal human approval and review gate.

`Human Review` uses the same structured waiting object as other human waits,
but remains a distinct tracker state and approval gate.

Signals or updates, normally submitted by a routed Coding Agent/operator flow
rather than App UI code:

- `approve_human_review`;
- `request_rework`;
- `submit_human_fix`;
- `cancel_issue_workflow`.

Activities:

- `HumanReviewValidationActivity` after approval or human fix.

Exit:

- to `Merging` when approved and lightweight validation passes;
- to `Rework` when the human requests changes;
- to `Need Human Input` when validation needs a human decision.

Notes:

- `Human Review` is a static tracker lane by default.
- Approval, request-rework, or human-fix actions arrive through
  Codex/operator flow and the Operator Action Bridge.
- The submitted action creates the executable condition for validation,
  `Rework`, or `Merging`.

## Rework

Purpose:

- implementation pass with review context.

Activities:

- `ReworkActivity`, or `MainAgentActivity` with explicit rework context;
- `ArtifactWriteActivity`.

Rework agent attempts follow `AGENT-ACTIVITY-CONTRACT.md` with review or
human-feedback refs in the request and addressed/pushed-back/unresolved
classification in the result.

Exit:

- to `Agent Review` when rework completes;
- to `Need Human Input` on blocker.

Notes:

- Rework is for review-driven implementation changes before merge.
- Merge-time semantic fixes should not bounce here by default.

## Merging

Purpose:

- land approved work.

Activities:

- `MergeActivity`;
- semantic fix behavior inside `MergeActivity` or a dedicated
  `MergeSemanticFixActivity`;
- `TrackerTransitionActivity`;
- `ArtifactWriteActivity`.

`MergeActivity` follows `AGENT-ACTIVITY-CONTRACT.md` with `merge_write`
capability and the configured land runner.

Exit:

- to `Done` when merge and terminal tracker updates succeed;
- to `Need Human Input` when merge, semantic fix, checks, permissions, or
  external services require human/operator action.

Notes:

- `Merging` may perform semantic fixes in place.
- If merge-time semantic fix cannot resolve the problem, move to
  `Need Human Input`, not `Rework`.
- The merging coding agent is not a separate Main handoff.

## Done

Purpose:

- terminal state.

Activities:

- terminal `TrackerTransitionActivity`;
- artifact finalization;
- claim cleanup.

Rules:

- no further workflow work after `Done`;
- any follow-up becomes a new issue/workflow.
