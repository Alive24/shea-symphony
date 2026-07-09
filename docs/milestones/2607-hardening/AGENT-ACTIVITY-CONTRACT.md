# Agent Activity Contract

Status: Draft

## Purpose

Define the 2607 boundary for Coding Agent, Review Agent, Merge Agent, and
Doctor work inside Temporal.

Agent Activities are coarse attempt boundaries. They are not model-turn
boundaries and should not rebuild Codex app-server's internal agent loop inside
Temporal.

## Generic Request

Recommended request shape:

```text
AgentActivityRequest {
  workflow_id
  run_id
  repo_id
  issue_ref
  activity_kind
  lane
  attempt_id
  worktree_ref
  agent_backend
  capability_profile
  prompt_template_ref
  context_refs
  artifact_write_policy
  heartbeat_policy
  timeout_policy
}
```

Rules:

- `worktree_ref` is assigned by the Workflow/Activity boundary.
- `capability_profile` is a hard runtime permission profile, not a prompt
  reminder.
- `context_refs` point to issue, artifact, review, or operator context. Do not
  pass large transcripts or diffs through Temporal payloads.
- `attempt_id` identifies retry, heartbeat, and artifact paths for one agent
  attempt. It is not the Workflow ID.

## Generic Result

Recommended result shape:

```text
AgentActivityResult {
  outcome
  summary
  artifact_refs
  evidence_refs
  event_log_ref
  transcript_ref?
  diff_ref?
  test_result_refs
  worktree_summary
  pr_ref?
  proposed_next_state?
  blocking_reason?
  retry_after?
}
```

`proposed_next_state` is only a proposal. Workflow decides the next state and
calls `TrackerTransitionActivity` when a tracker transition should be
committed.

Large transcripts, turn logs, diffs, test output, and review reports belong in
the artifact store. Activity result payloads carry summaries and references.

## Capability Profiles

Start with an enum, not a plugin permission engine:

- `read_only`
- `code_write`
- `merge_write`
- `review_read_only`
- `review_comment`
- `review_safe_autofix`
- `doctor_read`
- `doctor_write_safe`
- `doctor_write_operator`

Default mapping:

- `MainAgentActivity`: `code_write`
- `ReworkActivity`: `code_write`
- `AgentReviewActivity`: `review_read_only` or `review_comment`
- `HumanReviewValidationActivity`: `read_only`
- `MergeActivity`: `merge_write`
- automatic doctor: `doctor_write_safe` when the repair is bounded and
  idempotent, otherwise `doctor_read`
- operator-routed doctor: `doctor_read`, `doctor_write_safe`, or
  `doctor_write_operator` according to the submitted action context

Automatic doctor may write only safe, bounded runtime repairs:

- refresh cache;
- rebuild projection;
- clean stale local rows;
- retry idempotent link/readback repair.

If doctor does not know how to write safely, or the repair needs code changes,
tracker lane changes, destructive cleanup, permission changes, or human
judgment, it must move to `Need Human Input`.

## Activity Prohibitions

Agent Activities must not:

- directly write tracker lane state;
- directly update SQLite workflow indexes;
- decide final tracker state;
- create or choose the canonical worktree;
- put large transcripts, diffs, or review reports in Activity result payloads;
- use prompt text as the permission boundary.

## Worktree Lease

Write-capable Agent Activities use a lease, not just a path.

Recommended shape:

```text
WorktreeLease {
  lease_id
  repo_id
  issue_ref
  worktree_path
  branch_name
  base_ref
  owner_workflow_id
  owner_activity_id
  mode
  acquired_at
  expires_at?
}
```

Rules:

- `read_only` and `review_read_only` may not need an exclusive lease.
- `code_write`, `merge_write`, `review_safe_autofix`, `doctor_write_safe`, and
  `doctor_write_operator` require a lease.
- Activity heartbeat refreshes lease liveness.
- Activity result includes `worktree_summary`.
- Workflow decides whether a chained lane handler can reuse the same worktree.

## Heartbeat Layers

Heartbeat must be layered so operators can tell where work is stuck:

- `temporal_activity`: the Temporal Activity worker/wrapper is alive;
- `local_runner`: the local process or wrapper launching the agent is alive;
- `codex_session`: Codex app-server accepted or is running the session;
- `agent_run`: the agent-level task is progressing;
- `model_turn`: an optional detailed layer for model/tool-loop activity.

Shared heartbeat shape:

```text
HeartbeatSummary {
  layer
  status
  phase
  last_progress_at
  child_ref
  artifact_refs
  message
}
```

Common statuses:

- `starting`
- `queued`
- `running`
- `waiting`
- `succeeded`
- `failed`
- `cancelled`
- `stale`
- `unknown`

Temporal heartbeat stores only small current summaries and references. Complete
event streams belong in artifact/event logs or the agent backend.

## Timeout Policy

Use layered timeouts:

```text
TimeoutPolicy {
  schedule_to_start
  start_to_close
  heartbeat_timeout
  codex_queue_timeout
  no_progress_timeout
  model_turn_timeout?
}
```

Meanings:

- `schedule_to_start`: Temporal worker/queue delay;
- `start_to_close`: maximum Activity duration;
- `heartbeat_timeout`: local wrapper liveness;
- `codex_queue_timeout`: Codex app-server did not start the session;
- `no_progress_timeout`: agent run is alive but not making useful progress;
- `model_turn_timeout`: optional diagnostic timeout for current model/tool
  turn.

## Cancellation

Workflow cancellation does not prove the child agent session stopped.

Recommended result:

```text
CancellationResult {
  requested
  child_cancelled
  worktree_safe
  artifact_refs
  followup_required?
}
```

Activity should request child-session cancellation and report whether it was
confirmed. If cancellation cannot be confirmed safely, return `need_human_input`
or `conflict`; do not silently report success.

## Activity-Specific Contracts

Main:

- generates implementation;
- must return `pr_ref` or an explicit no-PR reason;
- returns worktree and validation summaries.

Rework:

- input must include review or human-feedback refs;
- output must classify feedback as addressed, pushed back, or unresolved.

Agent Review:

- supports configurable capabilities:
  - `review_read_only`
  - `review_comment`
  - `review_safe_autofix`
- verdicts:
  - `pass`
  - `pass_with_comments`
  - `safe_autofix_applied`
  - `request_rework`
  - `need_human_input`
  - `unhandled_error`

`review_safe_autofix` is bounded:

- only modifies the current PR/worktree;
- must return `diff_ref`;
- must run the configured validation subset;
- cannot merge;
- cannot advance tracker state;
- exits to `request_rework` or `need_human_input` when the fix exceeds safe
  scope.

Merge:

- runs the configured land runner;
- may perform semantic fix through `MergeActivity` or a dedicated merge-fix
  boundary;
- cannot bounce failed merge-time semantic fixes to `Rework` by default;
- unresolved merge, check, permission, or semantic-fix problems go to
  `Need Human Input` or `unhandled_error`.
