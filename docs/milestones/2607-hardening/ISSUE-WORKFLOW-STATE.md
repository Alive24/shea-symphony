# IssueWorkflow State

Status: Draft

## Purpose

Define the durable state shape for Temporal `IssueWorkflow`.

`IssueWorkflow` should store the small state needed to resume, decide, query,
and recover. It should not store full tracker payloads, workpads, diffs,
transcripts, review reports, or artifact bodies.

## Principle

Workflow state is durable control state:

- enough to know where the issue is;
- enough to resume after interruption;
- enough to decide the next Workflow step;
- enough for App queries to show useful summaries;
- enough to locate rich evidence by reference.

Rich evidence belongs outside Workflow state:

- tracker issue body, comments, and workpads;
- local artifacts;
- Codex transcripts and event streams;
- review reports;
- diffs and patches;
- full worktree status.

## Durable State Shape

Recommended state:

```text
IssueWorkflowState {
  workflow_id
  repo_id
  issue_ref
  tracker_backend
  current_tracker_state
  active_step
  active_attempt
  waiting
  last_transition
  active_agent_run
  active_review_run
  active_merge_run
  artifact_refs
  pr_summary
  human_todo_summary
  runtime_health_summary
}
```

Do not store:

- full issue description;
- full workpad body;
- full tracker comments;
- full diff;
- full transcript;
- full review report;
- full Project field dump;
- full worktree status.

## Tracker State

`current_tracker_state` means the last tracker state confirmed through
`TrackerTransitionActivity`.

It is not a replacement for the tracker as external fact.

Rules:

- normal Workflow decisions may use `current_tracker_state`;
- transition Activities perform targeted tracker readback;
- external tracker changes become typed conflicts or reconcile inputs;
- App queries should not fresh-scan the tracker on every refresh;
- dashboard displays may include freshness or readback timestamps when useful.

This keeps App refresh fast while preserving the rule that tracker state is the
external workflow fact.

## Attempt Summaries

Workflow state stores attempt summaries, not event streams.

Recommended run summary:

```text
RunSummary {
  attempt_id
  activity_id
  backend
  started_at
  last_progress_at
  status
  session_ref
  artifact_refs
}
```

Examples:

- `active_agent_run` for Main/Rework Codex attempts;
- `active_review_run` for Agent Review backend attempts;
- `active_merge_run` for merge and semantic-fix attempts.

Do not store every Codex app-server event, model turn, tool call, token usage
event, log line, or review finding in Workflow state. Store those in artifacts
or event logs and keep refs in the run summary.

## Waiting State

Human-facing waits should use one structured object.

Recommended shape:

```text
WaitingState {
  kind
  reason_enum
  reason_detail
  resume_target
  requested_action
  artifact_refs
  created_at
}
```

Allowed `kind` values:

- `need_to_clarify`;
- `need_human_input`;
- `human_review`.

This lets the App show one Human Todo surface while preserving state semantics:

- `Need to Clarify` is contract clarification before or during work;
- `Need Human Input` is a mid-workflow unblock state;
- `Human Review` is the formal approval gate.

## Last Transition

`last_transition` stores the most recent committed transition summary returned
by `TrackerTransitionActivity`.

Recommended shape:

```text
TransitionSummary {
  transition_id
  from_state
  to_state
  outcome
  reason_enum
  reason_detail
  committed_at
  tracker_backend
  artifact_refs
}
```

Failed transition attempts should be represented through Activity result,
Activity failure, or artifact refs. Do not inflate Workflow state with every
historical transition attempt; issue detail can load recent artifact refs when
needed.

## Artifact Refs

Artifact refs are first-class Workflow state. Artifact bodies are not.

Recommended shape:

```text
ArtifactRef {
  id
  kind
  path
  summary
  created_by_step
  created_at
}
```

Rules:

- every significant Activity result should produce or reference artifacts when
  the data is too large for Temporal history;
- App queries may return artifact refs and short summaries;
- App detail views may lazy-load artifact bodies;
- top-level dashboard queries should not read artifact bodies.

## Query Surfaces

Use two query layers.

### Dashboard Snapshot

`dashboard_snapshot` is the top-level App read model.

Recommended fields:

```text
DashboardIssueSummary {
  workflow_id
  issue_ref
  title
  current_state
  active_step
  human_todo_summary
  pr_summary
  health_summary
  last_progress_at
  artifact_ref_count
}
```

It should not include:

- full history;
- artifact bodies;
- worktree status;
- full tracker comments;
- full review reports;
- full transcripts.

### Issue Detail Snapshot

`issue_detail_snapshot` expands one issue after drill-down.

Recommended fields:

```text
IssueDetailSnapshot {
  dashboard_fields
  attempt_summaries
  last_transition
  waiting_detail
  recent_artifact_refs
  review_verdict_summary
  merge_summary
}
```

It may include more artifact refs, but should still lazy-load artifact bodies.

## Deliberately Not Chosen

Do not use the Workflow as a full issue cache.

What this gives up:

- App detail may need a targeted Activity read or artifact read for rich
  tracker context.
- Debugging may require opening artifact refs.
- Query output is not a complete historical archive.

Why this is acceptable:

- Temporal replay stays fast.
- Query payloads stay small.
- Tracker and artifact store remain the right homes for rich evidence.
- The dashboard avoids repeated heavy control-plane reads.

Do not derive dashboard truth in the App.

The App should query `IssueWorkflow`/`SymphonySnapshot` and render structured
state. It should not infer source-of-truth state from tracker comments,
artifact filenames, or local worktree inspection.

## Implementation Notes

- Workflow fields must be serializable and versionable.
- Additive state changes are preferred over renaming/removing fields.
- If a field grows large, replace it with an artifact ref.
- Query handlers should not perform filesystem, network, tracker, or artifact
  I/O.
- Activities perform targeted reads and writes; Workflows expose their current
  durable summary.
