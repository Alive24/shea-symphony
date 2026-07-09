# T2607-05 Agent Activity Boundary

Status: Draft

## Purpose

Implement the Temporal Activity boundary for coding, review, merge, rework, and
doctor work.

Agent Activities are coarse attempt boundaries. Temporal should know that an
agent attempt started, heartbeated, produced artifacts, proposed a next state,
or failed. Temporal should not model every model turn, tool call, or Codex
app-server internal event as a separate Workflow step.

## Inputs

This package implements decisions from:

- `AGENT-ACTIVITY-CONTRACT.md`;
- `RUNTIME-ROLE-MAPPING.md`;
- `CHILD-WORKFLOW-POLICY.md`;
- `ACTIVITY-ERROR-TAXONOMY.md`;
- `TEMPORAL-SPINE.md`;
- `OPERATOR-ACTION-BRIDGE.md`.

## Goals

- Move Main, Rework, Agent Review, Merge, Human Review validation, and Doctor
  attempts behind typed Temporal Activities.
- Preserve Codex app-server as the default coding runtime behind a coarse
  attempt boundary.
- Preserve review backends as review Activities that return typed verdicts.
- Enforce hard capability profiles at runtime.
- Require worktree leases for write-capable agent work.
- Store large transcripts, diffs, reports, logs, and patches as artifact refs.
- Provide layered heartbeats that locate where an attempt is stuck.
- Let agent outputs propose next state without committing tracker transitions.
- Keep Child Workflows as later promotion paths, not the default.

## Non-Goals

- No per-model-turn Temporal Activity graph.
- No rebuild of Codex app-server's internal agent loop inside Temporal.
- No prompt-only permission boundary.
- No direct tracker writes from agent Activities.
- No direct SQLite workflow index writes from agent Activities.
- No direct merge, tracker transition, or PR link from review/doctor tools
  unless routed through the correct Symphony Activity boundary.
- No large payloads in Workflow history.
- No broad MCP or raw Temporal client exposed to Coding Agents.

## Expected Code Areas

Recommended package shape:

```text
symphony/
  agent/
    activity.rs
    dto.rs
    capability.rs
    heartbeat.rs
    artifacts.rs
    worktree_lease.rs
    codex_backend.rs
    review_backend.rs
    merge_backend.rs
    doctor_backend.rs
```

Names are illustrative. Prefer existing repo modules when they already contain
working Codex app-server, review, merge, or doctor behavior.

## Activity Variants

Start with explicit variants over one shared request/result foundation:

```text
AgentActivity {
  Main(AgentActivityRequest)
  Rework(AgentActivityRequest)
  AgentReview(AgentActivityRequest)
  HumanReviewValidation(AgentActivityRequest)
  Merge(AgentActivityRequest)
  Doctor(AgentActivityRequest)
}
```

Separate Temporal Activity registrations are acceptable if that fits the Rust
SDK or existing code better. They should still share:

- request/result DTOs;
- capability profiles;
- heartbeat shape;
- artifact policy;
- worktree lease policy;
- outcome mapping.

## Request DTO

Recommended request:

```text
AgentActivityRequest {
  workflow_id
  run_id?
  repo_id
  issue_ref
  activity_kind
  lane
  attempt_id
  agent_backend
  capability_profile
  worktree_ref?
  prompt_template_ref
  context_refs
  artifact_root
  artifact_write_policy
  heartbeat_policy
  timeout_policy
  idempotency_key
}
```

Rules:

- `attempt_id` identifies one agent attempt, retry slot, and artifact path.
- `workflow_id` is the enclosing Temporal Workflow ID.
- `run_id` is optional and used only when exact Temporal execution lookup is
  needed.
- `context_refs` are pointers to issue contract, review feedback, operator
  input, artifacts, diffs, test summaries, or workpad sections.
- Do not pass full transcripts, large diffs, full issue comment history, or
  large review reports through Temporal payloads.
- `capability_profile` is enforced by runtime code, not only by prompt text.

## Result DTO

Recommended result:

```text
AgentActivityResult {
  outcome
  activity_kind
  attempt_id
  summary
  artifact_refs
  evidence_refs
  event_log_ref?
  transcript_ref?
  diff_ref?
  test_result_refs
  worktree_summary?
  pr_ref?
  review_verdict?
  doctor_findings?
  proposed_next_state?
  blocking_reason?
  retry_after?
  cancellation?
}
```

`proposed_next_state` is only a proposal. `IssueWorkflow` decides next state
and calls `TrackerTransitionActivity` when tracker state must change.

## Outcome Mapping

Use the normalized classes from `ACTIVITY-ERROR-TAXONOMY.md`:

- `success`;
- `already_applied`;
- `retryable`;
- `wait_and_retry`;
- `need_human_input`;
- `conflict`;
- `rejected`;
- `terminal_noop`;
- `unhandled_error`.

Do not let backend-specific status strings leak into Workflow routing. Map
Codex app-server, review backend, merge, and doctor outcomes at the Activity
boundary.

## Capability Profiles

Initial enum:

- `read_only`;
- `code_write`;
- `merge_write`;
- `review_read_only`;
- `review_comment`;
- `review_safe_autofix`;
- `doctor_read`;
- `doctor_write_safe`;
- `doctor_write_operator`.

Capability enforcement should gate:

- whether a worktree lease is required;
- whether file writes are allowed;
- whether push/PR operations are allowed;
- whether merge/land operations are allowed;
- whether tracker mutation APIs are unavailable;
- whether safe local repair operations are allowed;
- which artifact and context paths are readable.

Prompt text may explain these rules, but it is not the boundary.

## Default Capability Mapping

- `MainAgentActivity`: `code_write`;
- `ReworkActivity`: `code_write`;
- `AgentReviewActivity`: `review_read_only`, `review_comment`, or
  `review_safe_autofix`;
- `HumanReviewValidationActivity`: `read_only`;
- `MergeActivity`: `merge_write`;
- automatic doctor: `doctor_write_safe` only for bounded idempotent repairs,
  otherwise `doctor_read`;
- operator-routed doctor: `doctor_read`, `doctor_write_safe`, or
  `doctor_write_operator` according to `OperatorActionContext`.

`review_safe_autofix` is configurable. When enabled, it may make bounded edits
to the current PR/worktree, return `diff_ref`, and run configured validation.
It cannot merge, commit tracker state, or bypass Agent Review verdict rules.

## Worktree Lease Lifecycle

Write-capable Activities require a `WorktreeLease`.

Recommended lifecycle:

```text
IssueWorkflow requests or reuses lease
  -> WorktreeLeaseActivity acquires lease
  -> Agent Activity receives lease ref
  -> Agent Activity heartbeats lease liveness
  -> Activity returns worktree summary
  -> Workflow decides reuse, release, or cleanup
```

Write-capable profiles:

- `code_write`;
- `merge_write`;
- `review_safe_autofix`;
- `doctor_write_safe`;
- `doctor_write_operator`.

Read-only profiles may use a non-exclusive worktree ref or repository snapshot
when that is enough.

Lease violations should return `conflict` or `need_human_input`, not silently
write into an uncontrolled path.

## Heartbeat Layers

Implement layered heartbeat summaries:

- `temporal_activity`: Temporal wrapper is alive;
- `local_runner`: local process launching the agent is alive;
- `codex_session`: Codex app-server accepted or is running the session;
- `agent_run`: agent-level task is progressing;
- `model_turn`: optional detail for model/tool-loop progress.

Recommended shape:

```text
HeartbeatSummary {
  layer
  status
  phase
  last_progress_at
  child_ref?
  artifact_refs
  message
}
```

Persist small current summaries through Temporal heartbeat and SQLite
`activity_progress`. Store complete event streams in artifacts.

The App should be able to tell whether it is waiting on:

- Temporal scheduling;
- local worker/runner;
- Codex app-server queue/session;
- agent execution progress;
- model/tool-loop detail.

## Timeout Policy

Implement layered timeouts:

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

Timeout handling:

- Temporal worker delay maps to schedule/start timeout behavior;
- local runner or heartbeat loss maps to `retryable` when safe;
- Codex queue timeout maps to `wait_and_retry` or `need_human_input` according
  to capacity and backend health;
- no useful progress maps to `retryable`, `wait_and_retry`, or
  `need_human_input` according to attempt policy;
- malformed backend events map to `retryable` once if transport-like, otherwise
  `unhandled_error`.

## Artifact Policy

Each attempt writes under an attempt-specific artifact root, such as:

```text
~/.shea/artifacts/<workflow-id>/<attempt-id>/
```

Expected artifact categories:

- `transcript`;
- `event_log`;
- `diff`;
- `patch`;
- `test_result`;
- `review_report`;
- `doctor_report`;
- `screenshots`;
- `summary`.

Activity results carry refs and summaries only.

Artifact write failures:

- should fail fast as `unhandled_error` when the artifact is required for
  traceability;
- may return `need_human_input` when the local filesystem is unavailable or
  permissions are broken;
- must not be hidden by claiming agent success without evidence.

## Main Activity

`MainAgentActivity` performs implementation work.

Required inputs:

- issue contract or shaped task context;
- repo/worktree policy;
- allowed scope;
- prompt template ref;
- artifact root;
- validation expectations.

Required outputs:

- implementation summary;
- changed file summary or explicit no-change reason;
- `pr_ref` or explicit no-PR reason;
- validation/test refs;
- transcript/event refs;
- proposed next state.

The Activity may propose `Agent Review`, `Need to Clarify`, `Need Human Input`,
or another state. It must not commit the tracker transition itself.

## Rework Activity

`ReworkActivity` performs implementation changes from review or human feedback.

Required inputs:

- prior PR/worktree refs;
- review findings or human feedback refs;
- unresolved comments;
- acceptance criteria;
- artifact refs from prior attempt.

Required outputs:

- addressed feedback summary;
- pushed-back feedback summary, if any;
- unresolved feedback summary;
- validation/test refs;
- PR/worktree refs;
- proposed next state.

If feedback is impossible or unsafe to address, return `need_human_input` or
`rejected` with evidence rather than looping silently.

## Agent Review Activity

`AgentReviewActivity` evaluates the implementation.

Supported capability profiles:

- `review_read_only`;
- `review_comment`;
- `review_safe_autofix`.

Verdicts:

- `pass`;
- `pass_with_comments`;
- `safe_autofix_applied`;
- `request_rework`;
- `need_human_input`;
- `unhandled_error`.

`pass_with_comments` is not a tracker commit by itself. `IssueWorkflow` decides
whether comments are evidence, human-review context, or a reason to rework.

`safe_autofix_applied` must include:

- `diff_ref`;
- validation refs;
- review report ref;
- statement that the fix stayed within configured safe scope.

If safe autofix exceeds scope, return `request_rework` or `need_human_input`.

## Human Review Validation Activity

This Activity validates human review or human-fix handoff before executable
work continues.

It should check:

- PR still exists;
- branch and PR evidence still match issue context;
- required checks pass or are explicitly accepted;
- diff since last agent review is summarized;
- human modification is acknowledged;
- unresolved review comments are resolved or explicitly deferred.

It is read-only by default. It may propose `Merging`, `Rework`, `Agent Review`,
or `Need Human Input`.

## Merge Activity

`MergeActivity` owns land/merge attempt work behind `merge_write`.

It may:

- run configured land runner;
- verify checks and branch state;
- perform bounded semantic fix inside merge flow when configured;
- push or update the PR branch when policy allows;
- read back merge/terminal facts.

It may not:

- write tracker `Done` directly;
- bypass `TrackerTransitionActivity`;
- route failed merge-time semantic fix to `Rework` by default.

Unresolved merge, check, permission, or semantic-fix problems should route to
`Need Human Input` or `unhandled_error` according to the failure.

## Doctor Activity

Automatic doctor may be read-only or safe-write.

Allowed safe writes for `doctor_write_safe`:

- refresh tracker cache;
- rebuild SQLite projection;
- clean stale local rows;
- retry idempotent link/readback repair;
- repair local artifact index when source artifacts are intact.

Not allowed for automatic doctor:

- code changes;
- tracker lane changes;
- destructive cleanup;
- permission changes;
- merge/land;
- ambiguous manual judgment.

If doctor does not know how to write safely, it should return
`need_human_input` with a concrete requested action.

Operator-routed doctor uses `doctor_write_operator` only through
`OperatorActionContext` and the narrow submit bridge.

## Cancellation

Activity cancellation should request cancellation from the child backend and
report whether it was confirmed.

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

Do not claim clean cancellation if the Codex session, merge runner, or local
process may still be mutating the worktree.

## SQLite Projection

Project small observable summaries through `LocalStateProjector`:

- activity kind;
- attempt id;
- status;
- heartbeat layer summaries;
- last progress timestamp;
- retry-after timestamp;
- artifact refs;
- current backend child ref.

SQLite is not the attempt ledger. Temporal history plus artifacts are the
durable trace. Projection failures should mark read-model freshness stale or
failed without rewriting tracker state.

## Migration Steps

### AAA-1: Shared DTOs And Capability Profiles

- Define `AgentActivityRequest`.
- Define `AgentActivityResult`.
- Define capability profile enum.
- Define heartbeat and timeout DTOs.
- Define artifact ref DTOs.

### AAA-2: Worktree Lease Boundary

- Implement write-capable lease requirement.
- Attach lease liveness to heartbeat.
- Return worktree summary from write-capable Activities.
- Prevent writes without a valid lease.

### AAA-3: Codex Main/Rework Backend

- Move current Main lane loop behind `MainAgentActivity`.
- Move rework behavior behind `ReworkActivity`.
- Preserve Codex app-server event normalization.
- Store transcripts and event logs as artifacts.

### AAA-4: Review Backend

- Move automatic review behind `AgentReviewActivity`.
- Support read-only, comment-only, and safe-autofix configuration.
- Return typed verdicts and artifact refs.

### AAA-5: Merge And Doctor Backend

- Move merge lane behavior behind `MergeActivity`.
- Preserve semantic fix policy inside merge flow.
- Move automatic doctor checks and safe repairs behind `DoctorActivity`.

### AAA-6: Delete Direct Mutation Paths

- Verify agent Activities cannot write tracker lanes.
- Verify agent Activities cannot update SQLite workflow indexes directly.
- Verify review/doctor cannot bypass capability profiles.
- Remove old lane-loop orchestration once Temporal paths cover behavior.

## Acceptance Checks

- Main/Rework/Merge/Review/Doctor attempts run as coarse Activities.
- Temporal history contains small request/result summaries and artifact refs,
  not large transcripts or diffs.
- Capability profiles are enforced outside prompt text.
- Write-capable Activities require worktree leases.
- Layered heartbeats identify Temporal, local runner, Codex session, agent run,
  and optional model-turn progress.
- Activity outcomes map into the shared error taxonomy.
- Agent outputs can propose next state but cannot commit tracker transitions.
- Agent Review supports configurable `review_safe_autofix`.
- Automatic doctor can perform bounded safe writes and routes uncertainty to
  `Need Human Input`.
- Cancellation reports whether the child backend actually stopped.

## Done Means

- Agent Activity DTOs and capability profiles exist;
- Codex Main/Rework are behind Activities;
- Agent Review is behind an Activity with typed verdicts;
- Merge is behind an Activity and preserves semantic-fix policy;
- Doctor is behind an Activity with safe-write limits;
- worktree leases and layered heartbeats are implemented;
- direct agent tracker/write-model mutation paths are deleted or blocked.
