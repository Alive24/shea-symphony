# Snapshot And Dashboard

Status: Draft

## Purpose

Define what the App reads by default, what Temporal-backed Symphony workflows
must expose, and where issue-level detail begins.

The App should be a lightweight operational surface. It should not become a
second tracker, a hidden runtime controller, or an eager artifact reader.

## Dashboard Scope

The top-level dashboard should focus on the current operational surface:

- active lane items;
- human todo items, including `Need to Clarify`, `Need Human Input`, and
  `Human Review`;
- current tracker state;
- PR number and concise PR state when relevant;
- whether workflow execution is running, paused, waiting, or terminal;
- whether local Temporal service and Symphony worker are available;
- whether a refresh/cache operation is stale, fresh, or failed.

The dashboard does not need to show historical project queues. Operators can
use the tracker for broad project history and long-tail queue browsing.

The dashboard also does not need worktree path, branch name, trace detail, full
logs, or artifact content. Those belong in lane item detail.

## Lane Item Detail

Issue-level detail may show:

- worktree path;
- branch name;
- PR URL and detailed PR status;
- current workflow step, with future graph node when available;
- recent transition attempts;
- recent error evidence;
- trace id and artifact references;
- workpad and review artifacts;
- doctor output when explicitly opened.

Detail pages may lazy-load artifacts after the operator drills down. Top-level
refresh should not read full transcripts, logs, review reports, or trace
payloads.

## Snapshot Shape

`SymphonySnapshot` should be the first read surface for the App. In 2607 it
should be backed by the local Symphony runtime boundary, not CLI command
output.

The read source depends on scope:

- single issue detail should prefer Temporal Query for authoritative workflow
  state;
- top-level multi-issue dashboard should prefer SQLite materialized snapshots;
- explicit refresh operations may run Activities that update tracker cache,
  PR summaries, and artifact indexes.

`IssueWorkflow` state and query layering are defined in
`ISSUE-WORKFLOW-STATE.md`.

It should include:

- snapshot id;
- generated timestamp;
- source timestamps for tracker and local runtime state;
- freshness/staleness markers;
- active issue summaries;
- human todo summaries;
- active PR numbers and concise PR state;
- workflow execution state;
- Temporal workflow state needed for display;
- artifact references and short summaries, not full artifact bodies;
- stuck or waiting classification when already represented as workflow state.

It should not include:

- full project history;
- full artifact contents;
- raw transcript bodies;
- full worktree status for every issue;
- source-of-truth state inferred by the App.

Use two read layers:

- `dashboard_snapshot` for lightweight operational summaries;
- `issue_detail_snapshot` for one issue's attempt summaries, waiting detail,
  recent artifact refs, review summary, and merge summary.

`dashboard_snapshot` is normally assembled from SQLite local read-model rows.
`issue_detail_snapshot` is normally backed by Temporal Query and may include
SQLite artifact index metadata. Both return artifact refs and summaries, not
artifact bodies.

## Runtime And Tracker State

Tracker state is the external workflow fact. Runtime state is local execution
evidence.

`IssueWorkflow.current_tracker_state` is the last tracker state confirmed by
`TrackerTransitionActivity`, not a replacement for tracker state as external
fact. Targeted readback happens in Activities, not every App refresh.

During lane handoff, `IssueWorkflow` must treat successful
`TrackerTransitionActivity` completion as part of completion. A worker should
not be considered fully handed off if an Activity finished but the tracker did
not move.

When runtime state and tracker state conflict:

- Symphony stops or enters a reconcile path;
- Symphony does not silently guess the next state;
- App displays the conflict as a read-only condition;
- if human attention is needed, Symphony should move the item into
  `Need Human Input` or `Need to Clarify` rather than leaving the issue stuck
  behind UI-only text.

## App Operations

The App should call Tauri backend commands that use Temporal client APIs:

- start workflow;
- query snapshot;
- query issue detail;
- send signal;
- send update when synchronous accepted/rejected feedback is needed.

The App should not directly trigger:

- tracker state mutation;
- worktree edits;
- manual worktree opening as a workflow operation;
- agent resume as an operator-side imperative outside Temporal;
- doctor repair flows that modify state outside Temporal workflow policy.

Doctor work may have two modes:

- automatic doctor checks as Activities;
- human doctor work opened through Codex/operator flow when coding-agent help is
  required.

## Stuck State Display

The dashboard should make stuck or waiting state visible through structured
fields, not through hidden interpretation:

- `waiting_on`;
- `blocked_reason`;
- `last_progress_at`;
- `last_transition_attempt`;
- `last_transition_error`;
- `recommended_next_action`.

If the recommended next action requires human attention, the tracker state
should reflect that through `Need Human Input`, `Need to Clarify`, or
`Human Review`.

## MVP Transport

For 2607 hardening, prefer:

- Temporal Query-backed issue detail reads;
- SQLite-backed dashboard materialized reads;
- local artifact references for large details;
- no independent local Symphony service in 2607.

The Tauri backend command layer is the local API surface for the App.

## Performance Bias

Top-level dashboard refresh should be cheap:

- avoid full project history scans;
- avoid full artifact reads;
- avoid full worktree scans;
- avoid mutating commands;
- avoid repeated tracker reads inside one refresh;
- prefer SQLite materialized cache plus explicit refresh when needed.

Hard timing targets should come after a measurement pass, but the first design
constraint is clear: local refresh should not feel blocked after LLM work has
already completed.
