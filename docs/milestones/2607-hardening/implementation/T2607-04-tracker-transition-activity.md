# T2607-04 TrackerTransitionActivity

Status: Draft

## Purpose

Implement the single Symphony-owned Activity boundary for tracker state writes
and tracker-visible durable mutations.

`IssueWorkflow` decides that a transition or tracker mutation is needed.
`TrackerTransitionActivity` commits it, verifies it by readback, and returns a
typed result. No lane code, App command, CLI command, extension, or Coding Agent
tool should write tracker state directly.

## Inputs

This package implements decisions from:

- `TRACKER-TRANSITION-ACTIVITY.md`;
- `TRACKER-TRANSITIONS.md`;
- `ACTIVITY-ERROR-TAXONOMY.md`;
- `TEMPORAL-SPINE.md`;
- `OPERATOR-ACTION-BRIDGE.md`.

## Goals

- Make Symphony the sole owner of tracker state writes.
- Preserve and migrate proven existing tracker adapter behavior.
- Make tracker state transitions durable, retryable, idempotent, and
  observable through Temporal.
- Require readback verification before reporting success.
- Move PR-to-issue linking into the same durable Activity boundary.
- Reduce GitHub Project field churn by keeping detailed runtime state local.
- Route conflicts back to `IssueWorkflow` rather than hidden mutation paths.
- Delete or reduce old lane/App/CLI mutation paths after equivalent Activity
  paths exist.

## Non-Goals

- No wrapper around the old autopilot or lane mutation model as target
  architecture.
- No direct tracker writes from App/Tauri, CLI product commands, extensions, or
  Coding Agent tools.
- No full `TrackerIssue` payload in Workflow history as the primary Activity
  contract.
- No dedicated tracker custom field only for idempotency.
- No SQLite tracker mutation ledger in the initial schema.
- No Activity-side business decision about whether a conflict should become
  `Need Human Input`, stop, or reconcile.

## Expected Code Areas

Recommended package shape:

```text
symphony/
  tracker/
    transition_activity.rs
    mutation_activity.rs
    dto.rs
    idempotency.rs
    readback.rs
    evidence.rs
    conflict.rs
    adapter.rs
```

Names are illustrative. Prefer the existing tracker adapter module layout if it
already has a clearer home for GitHub Project v2 and future tracker backends.

## Preserve Existing Strengths

Migrate proven behavior into the Activity boundary instead of rewriting it from
scratch:

- `TrackerAdapter`;
- GitHub Project v2 field reads/writes;
- tracker recovery readback semantics;
- recovery markers;
- workpad/timeline evidence helpers;
- event log audit records;
- existing project state failure classification;
- future tracker adapter boundary assumptions.

The owner changes. The low-level tracker client code does not need to be
discarded if it is already reliable.

## Activity Kinds

Start with one `TrackerTransitionActivity` boundary and explicit request
variants:

```text
TrackerTransitionActivityRequest {
  Transition(TrackerTransitionRequest)
  LinkPrToIssue(LinkPrToIssueRequest)
  WriteEvidence(TrackerEvidenceRequest)
  UpdateClaim(TrackerClaimRequest)
}
```

It is acceptable to expose separate registered Temporal Activity functions if
the Rust SDK or codebase makes that simpler, but they should share the same
typed result model, idempotency rules, readback behavior, and error taxonomy.

Do not let incidental shell commands or post-agent hooks perform these writes
outside the boundary.

## Transition Request

Recommended request:

```text
TrackerTransitionRequest {
  workflow_id
  run_id?
  issue_ref
  expected_from_state
  requested_to_state
  transition_kind
  requester
  reason_enum
  reason_detail
  evidence_refs
  idempotency_key
  field_update_intents
}
```

Required fields:

- `workflow_id`;
- `issue_ref`;
- `expected_from_state`;
- `requested_to_state`;
- `transition_kind`;
- `requester`;
- `reason_enum`;
- `idempotency_key`.

Optional fields should stay small and summary-oriented. Large evidence belongs
in artifacts, workpads, tracker comments, or targeted issue detail reads.

## Transition Result

Recommended result:

```text
TrackerTransitionResult {
  outcome
  issue_ref
  from_state_observed
  to_state_committed?
  tracker_backend
  tracker_revision?
  evidence_refs
  audit_ref?
  conflict_reason?
  retry_after?
  message
}
```

Initial outcome enum:

- `committed`;
- `already_applied`;
- `conflict`;
- `rejected`;
- `retry_later`;
- `need_human_input`;
- `unhandled_error`.

Use Temporal Activity retry failures for clearly transient infrastructure
failures when automatic retry is appropriate. Return a typed outcome when the
Workflow needs to make a durable routing decision.

## Idempotency

Every side-effecting request needs a stable idempotency key.

Recommended transition key ingredients:

- workflow id;
- issue ref;
- transition kind;
- expected from state;
- requested to state;
- attempt slot.

Example:

```text
transition:<workflow-id>:<issue-ref>:<from-state>:<to-state>:<transition-kind>:<attempt-slot>
```

The key should be stable across Activity retries for the same intended side
effect. It should change when the Workflow intentionally attempts a different
transition.

Activity retry must not duplicate:

- tracker comments;
- workpad/timeline evidence;
- claims;
- project field writes;
- PR-to-issue links;
- terminal writes.

Use existing recovery markers where they already solve duplicate evidence
writes. Do not add a new GitHub Project field only to store idempotency keys.

## Readback Verification

Activity success means the desired external fact was observed.

For state transitions, read back:

- current tracker state;
- relevant project field values;
- evidence marker or audit marker when evidence was requested.

For PR link mutation, read back:

- issue relation to PR;
- PR number/state summary when available;
- any tracker-visible PR field that operators rely on.

For claim/field updates, read back:

- claim owner/status;
- lane owner/status;
- expected human-visible fields.

If write succeeds but readback cannot confirm the desired fact:

- return `retry_later` for eventual-consistency or provider-delay cases;
- return `conflict` when readback proves a different external fact;
- return `need_human_input` for auth/schema/permission problems;
- return `unhandled_error` for malformed internal requests.

Do not report `committed` from an API success alone.

## PR-To-Issue Link Mutation

PR linking is a first-class tracker mutation, not an incidental shell command.

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
  idempotency_key
}
```

Recommended stable idempotency key:

```text
link-pr:<repo-id>:<issue-number>:<pr-number>
```

Outcomes:

- relation already exists: `already_applied`;
- relation written and read back: `committed`;
- write may have landed but readback is delayed: `retry_later`;
- auth, permission, or missing scope: `need_human_input`;
- issue/PR mismatch or wrong repo: `conflict`;
- malformed internal request: `unhandled_error`.

Do not rely on PR body closing keywords as the only relation signal. They may
be useful evidence, but the Activity must verify the relation Symphony needs.

## Evidence Writes

Every committed transition should leave enough evidence for an operator to
answer:

- why did the state change?
- who or what requested it?
- what workflow execution committed it?
- what artifacts support the decision?
- what can be retried if it gets stuck?

Minimum evidence summary:

```text
TransitionEvidence {
  workflow_id
  run_id?
  issue_ref
  from_state
  to_state
  transition_kind
  requester
  reason_enum
  reason_detail?
  artifact_refs
  committed_at
}
```

Large payloads stay out of tracker fields and Temporal history. Store refs.

## Tracker Field Diet

Keep tracker-visible fields for human-visible facts:

- current workflow state;
- coarse lane owner/status when useful outside the App;
- PR number/status/relation summary;
- terminal result or blocker reason;
- high-signal evidence refs.

Move local runtime details out of tracker fields:

- worker attempt ids;
- Codex session or turn details;
- heartbeat timestamps;
- local worktree paths;
- retry counters;
- detailed recovery machinery;
- full diagnostics, transcripts, reports, or diffs.

If local state for one issue becomes unrecoverable, the recovery path is to stop
that issue, clear local runtime state, and restart from tracker state plus
durable artifacts. Do not keep writing every recovery detail to GitHub Project
fields just in case.

## Conflict Handling

The Activity reports observed facts. `IssueWorkflow` decides policy.

Recommended conflict reasons:

- `external_state_changed`;
- `external_terminal_state`;
- `expected_state_missing`;
- `project_schema_changed`;
- `pr_relation_conflict`;
- `claim_conflict`;
- `permission_or_scope_missing`;
- `malformed_request`;
- `readback_inconsistent`.

Examples:

- observed state equals requested state: return `already_applied`;
- observed state differs from `expected_from_state`: return `conflict`;
- observed state is terminal: return `conflict` with
  `external_terminal_state`;
- auth or missing project field capability: return `need_human_input`;
- malformed Symphony-generated DTO: return `unhandled_error`.

Do not silently continue through conflicting external tracker changes.

## Retry Policy

Use Temporal Activity retry for transient infrastructure failures:

- network errors;
- HTTP 5xx;
- temporary tracker backend outage;
- retryable command transport failure.

Use durable wait/retry outcomes for provider pacing:

- GitHub rate limits;
- eventual consistency after a write;
- quota windows with known retry time.

Do not automatically retry:

- missing credentials;
- missing tracker permissions;
- schema drift requiring human action;
- semantic conflicts;
- malformed internal payloads;
- destructive operations without readback/idempotency.

## SQLite Projection

After readback confirms the tracker fact, project observable summaries into
SQLite through `LocalStateProjector`:

- `tracker_cache.tracker_state`;
- PR number/state/relation confirmation timestamp;
- relevant freshness status;
- `activity_progress` summary;
- artifact refs.

SQLite projection is not the commit. If projection fails after tracker commit,
the Activity should return the tracker result and mark projection stale or emit
a local projection error according to policy. Do not repeat the tracker write
only because SQLite projection failed.

## App, CLI, And Extension Boundaries

The App may request workflow actions through Tauri backend commands that call
Temporal start/query/signal/update boundaries. It must not call tracker
mutation APIs directly.

CLI product commands should not write tracker state directly. Debug/admin
commands, if kept, should call the same Temporal or Activity boundary.

Extensions and Coding Agent tools may propose transitions or submit structured
operator actions. They must not write tracker fields, close issues, clear
claims, link PRs, or merge PRs directly.

## Migration Steps

### TTA-1: Contract And Tests

- Define request/result DTOs.
- Define transition outcomes and conflict reasons.
- Define idempotency key helpers.
- Port recovery marker/readback tests into the new boundary.
- Add tests proving full `TrackerIssue` is not required in Workflow contracts.

### TTA-2: State Transition Commits

- Move all tracker lane/status changes into `TrackerTransitionActivity`.
- Cover `Backlog`, `Todo`, `Need to Clarify`, `In Progress`,
  `Need Human Input`, `Agent Review`, `Human Review`, `Rework`, `Merging`, and
  `Done`.
- Remove direct lane/App/CLI state writes.

### TTA-3: PR Link And Evidence Commits

- Move PR-to-issue linking into the Activity boundary.
- Move transition evidence writes into the Activity boundary.
- Preserve workpad/timeline marker semantics.
- Store large evidence as artifact refs.

### TTA-4: Claim And Field Diet

- Audit current GitHub Project fields.
- Keep human-visible coarse fields.
- Move detailed worker/session/retry state into Temporal state,
  `activity_progress`, and artifacts.
- Stop writing redundant field flags after compatibility is no longer needed.

### TTA-5: Reconcile And Recovery

- Implement typed conflict handling for external tracker changes.
- Implement readback recovery for transient write uncertainty.
- Define one-issue local reset recovery.
- Route non-retryable auth/schema/config failures to `Need Human Input`.

### TTA-6: Delete Old Mutation Paths

- Delete or reduce old lane mutation paths.
- Delete or reduce old CLI product mutation paths.
- Verify App/Tauri has no tracker mutation bypass.
- Verify extensions and Coding Agent tool surfaces cannot mutate tracker state.

## Acceptance Checks

- Every standard state transition goes through `TrackerTransitionActivity`.
- Activity request/result DTOs stay small and versionable.
- Full tracker issue payloads are not written into Workflow history as the
  transition contract.
- Every side-effecting request has an idempotency key.
- State transition success requires readback-confirmed target state.
- PR link success requires readback-confirmed relation.
- Duplicate Activity retry does not create duplicate comments, evidence,
  claims, PR links, or terminal writes.
- External tracker state conflicts return typed conflict outcomes.
- Auth/schema/permission failures route to `need_human_input`.
- SQLite projection failure does not repeat a successful tracker write.
- App, CLI product commands, lanes, extensions, and Coding Agent tools have no
  direct tracker write path.

## Done Means

- `TrackerTransitionActivity` owns tracker state writes;
- PR-to-issue linking is inside the same durable write boundary;
- readback/idempotency/recovery markers are migrated;
- field diet is applied or explicitly tracked for deletion;
- old mutation paths are deleted or reduced to the same Temporal boundary;
- tests prove retry safety and direct-write prevention.
