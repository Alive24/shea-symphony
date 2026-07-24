# TrackerTransitionActivity Contract

Status: Partially implemented — #494 completed the inert state-transition contract slice

## Implemented Contract Slice (#494)

The first durable payload boundary is now complete: internal serialized
request/result DTOs, validated opaque tracker-state strings, closed
Symphony-owned outcome/conflict enums, and a versioned deterministic
idempotency key. The registered `TrackerTransitionActivity` keeps its durable
name and core-queue registration, but returns an explicit inert result and
performs no tracker or other external side effect.

The following mutation slices remain explicitly deferred: adapter invocation
and readback, Project failure/retry mapping, PR linking, evidence and
workpad/timeline commits, claims, generic field updates, SQLite projection, and
Workflow scheduling/routing.

## Purpose

Define the 2607 contract for moving tracker state writes into Temporal without
rebuilding tracker clients or preserving the old lane loop as a wrapper.

`TrackerTransitionActivity` is the single commit authority for tracker state
transitions. Existing tracker adapter capabilities should migrate into this
Activity boundary rather than being wrapped by a legacy facade.

## Non-Wrapper Decision

Do not keep a compatibility wrapper around the old autopilot/lane mutation
model as the target architecture.

The migration should reuse existing proven code:

- `TrackerAdapter`;
- tracker recovery readback semantics;
- recovery markers;
- workpad/timeline evidence helpers;
- event log audit records;
- GitHub Project v2 and future tracker adapter boundaries.

But the owner changes: `IssueWorkflow` requests a transition and
`TrackerTransitionActivity` commits it. Lane code, App commands, CLI commands,
and Shea extensions stop writing tracker state directly.

## Request Shape

Use a small Temporal payload. Do not pass full `TrackerIssue` values through
Workflow history as the primary contract.

Recommended request:

```text
TrackerTransitionRequest {
  workflow_id
  run_id?
  issue_ref { tracker_kind, repository, issue }
  expected_from_state
  requested_to_state
  transition_kind
  requester
  reason { code, detail? }
  evidence_refs?
  attempt_slot
  idempotency_key
}
```

`issue_ref` holds only stable tracker identity, never a rich issue or Project
payload. Future generic field-update intents are a separate mutation slice.

## Result Shape

Recommended result:

```text
TrackerTransitionResult {
  outcome
  issue_ref
  observed_from_state?
  committed_to_state?
  evidence_refs?
  audit_ref?
  conflict_reason?
  retry_after_ms?
  summary
}
```

Outcomes:

- `committed`;
- `already_applied`;
- `conflict`;
- `rejected`;
- `retry_later`.
- `need_human_input`;
- `unhandled_error`.

## Deliberately Not Chosen

Do not use full `TrackerIssue` as the Activity request/result contract.

What this gives up:

- Workflow history will not contain the complete issue description, workpad,
  comments, project fields, linked PR payloads, or rich tracker evidence.
- Debugging a transition may require opening artifact refs or querying the
  tracker again.
- Activity internals still need targeted tracker reads.

Why this is acceptable:

- Temporal history stays small and replay-friendly.
- Activity contracts remain versionable.
- App dashboard queries can stay fast.
- Rich evidence already belongs in artifacts, workpads, tracker comments, or
  targeted issue detail reads.

Do not introduce a dedicated tracker custom field only for idempotency.

What this gives up:

- There is no single always-visible Project field that lists every committed
  transition key.

Why this is acceptable:

- Existing recovery markers and readback checks already cover retry safety.
- Extra Project fields made the MVP harder to read and slower to refresh.
- Local Temporal history and artifact state are better places for detailed
  retry machinery.

Do not model every tracker-side field update as Workflow state.

What this gives up:

- The Workflow will not expose every lane claim field and project field as
  first-class durable fields.

Why this is acceptable:

- Project fields should show human-visible workflow facts.
- Detailed worker/session/attempt state belongs in Temporal workflow state,
  Activity heartbeat/progress, and local artifacts.

## Idempotency

Use stable idempotency keys for every side-effecting transition.

Recommended key ingredients:

- workflow id;
- issue ref;
- transition kind;
- expected from state;
- requested to state;
- attempt slot.

The implemented `symphony.transition.v1` format length-prefixes every
fixed-order component. It is unambiguous even when opaque tracker strings
contain delimiters, and the Workflow carries the result into each Activity
retry instead of regenerating it inside the Activity.

Existing mechanisms should be migrated, not replaced:

- state transitions use tracker readback equality;
- project field updates use field equality readback;
- workpad and timeline evidence use hidden recovery markers;
- artifacts use transition ids in paths;
- event log audit records include the transition id or idempotency key.

Activity retry must not create duplicate tracker comments, duplicate claims,
duplicate worktrees, duplicate PR links, or duplicate terminal writes.

## Durable Tracker Mutations

Some tracker writes are not state transitions, but they are still durable
side-effect intents. PR-to-issue linking is the first important case.

Represent these as explicit tracker mutation kinds inside the same Activity
boundary rather than as incidental post-PR shell commands.

First required mutation kind:

```text
TrackerMutationKind::LinkPrToIssue
```

Recommended request:

```text
LinkPrToIssueRequest {
  mutation_id
  workflow_id
  repo_id
  issue_ref
  pr_ref
  desired_relation
  evidence_refs
}
```

Recommended stable idempotency key:

```text
link-pr:<repo-id>:<issue-number>:<pr-number>
```

The Activity must write and then read back. Success means the desired PR
relation is confirmed by tracker readback, not merely that an API call or `gh`
command exited successfully.

Outcomes should map into the shared Activity taxonomy:

- relation already exists: `already_applied`;
- write succeeds but readback does not show the relation yet:
  `wait_and_retry`;
- network, rate-limit, or transient backend failure: `retryable` or
  `wait_and_retry`;
- missing permission or auth scope: `need_human_input`;
- issue/PR repo mismatch: `conflict`;
- malformed internal request: `unhandled_error`.

Do not rely on PR body closing keywords as the only reliable relation signal.
They may be written as useful evidence, but the Activity must verify the
relation that Symphony needs to observe.

Do not add a dedicated SQLite mutation log table for this in the first schema.
Temporal history is the durable attempt ledger. SQLite projects current
observability through `activity_progress`, confirmed PR fields in
`tracker_cache`, and artifact refs.

## Tracker Field Diet

2607 should reduce unnecessary GitHub Project field churn.

Project/tracker fields should retain:

- current workflow state;
- coarse lane ownership when human-visible;
- PR/status facts that operators need in the tracker;
- terminal result or blocker facts that are useful outside the App.

Local Temporal state and artifacts should own:

- worker attempt ids;
- heartbeat/progress timestamps;
- Codex thread/session references;
- detailed recovery state;
- local worktree paths;
- retry counters;
- transient diagnostics;
- large evidence and transcripts.

If local state becomes unrecoverable for one issue, the acceptable recovery
strategy is to stop that issue, clear local state, and restart from tracker
state and durable artifacts. Each issue is atomic enough that this is cheaper
than keeping every runtime detail in GitHub Project fields.

## External Tracker Changes

Tracker state is the external fact. Runtime state is local execution evidence.

Recommended behavior:

- if observed state already equals requested target, return `already_applied`;
- if observed state differs from `expected_from_state` and is active, return
  `conflict` with reason `external_state_changed`;
- if observed state is terminal, return `conflict` with reason
  `external_terminal_state`;
- if the tracker operation hits network, rate-limit, or transient backend
  failure, return retryable Activity failure or `retry_later` according to the
  Temporal retry policy;
- if auth, schema, permission, or validation is broken, return non-retryable
  failure so `IssueWorkflow` can enter `Need Human Input`.

`IssueWorkflow` decides whether a conflict moves to `Need Human Input`, stops,
or reconciles. The Activity reports the fact; it does not guess the workflow
policy.

General Activity failure classes are defined in
`ACTIVITY-ERROR-TAXONOMY.md`. Tracker-specific `ProjectStateFailureKind` values
should map into that taxonomy instead of inventing a separate retry policy.

## Complete Migration Submilestones

The 2607 target is complete tracker transition ownership, not a partial
delivery milestone. To avoid losing scope, split the work into submilestones
with explicit coverage.

### TTA-1: Contract And Tests

The DTO/idempotency portion is complete in #494:

- [x] Define compact request/result DTOs.
- [x] Define transition outcomes and conflict reasons.
- [x] Define versioned deterministic idempotency-key construction.
- [x] Add serialization tests proving a full `TrackerIssue` is not required.
- [ ] Port recovery marker/readback tests when state commits exist.

### TTA-2: State Transition Commits

- Move all tracker status changes into `TrackerTransitionActivity`.
- Cover Main, Review, Human Review, Rework, Merging, Backlog, Todo, Need to
  Clarify, Need Human Input, and Done transitions.
- Remove direct lane/App/CLI state writes.

### TTA-3: Evidence And Workpad Commits

- Move transition evidence writes into the Activity boundary.
- Reuse workpad/timeline marker semantics.
- Keep large evidence in local artifacts and store refs in tracker-visible
  evidence.

### TTA-4: Claim And Field Diet

- Audit existing GitHub Project fields.
- Keep only human-visible coarse fields in the tracker.
- Move detailed worker/session/retry state into Temporal state, Activity
  progress, and local artifacts.
- Remove or stop writing redundant field flags after a compatibility window.

### TTA-5: Reconcile And Recovery

- Implement typed conflict handling for external tracker changes.
- Implement readback recovery for transient write failures.
- Define one-issue local reset recovery: clear local runtime state and rebuild
  from tracker state plus durable artifacts when needed.

### TTA-6: Delete Old Mutation Paths

- Delete or reduce old CLI/lane mutation code after equivalent Temporal paths
  exist.
- Keep debug/admin commands only if they call the same Temporal
  start/query/signal/update boundary.
- Verify no non-Activity path can write tracker state.
