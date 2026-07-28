# T2607-03 Workflow Coordinator

Status: Draft

## Purpose

Implement the thin launcher and registrar that turns executable tracker states
into Temporal `IssueWorkflow` executions.

The Coordinator is not a scheduler, not an agent runner, and not a workflow
decision engine. It is the boundary that answers:

- should this tracker issue have an active workflow execution now?
- if yes, what is the deterministic human-readable `workflow_id`?
- did Temporal actually start or already have the execution?
- did the local SQLite `workflow_index` record the active execution correctly?

## Inputs

This package implements decisions from:

- `WORKFLOW-ACTIVATION.md`;
- `TEMPORAL-CONCURRENCY.md`;
- `TASK-QUEUES.md`;
- `LOCAL-STATE-DB.md`;
- `adr/0006-temporal-local-runtime-spine.md`.

## Implementation Slices And Deferred Work

Issue #501 is the first independently reviewable slice. It owns only the
crate-private pure activation and Workflow identity contract: request
validation, optimistic expectation comparison, state classification, target
derivation, provenance, encoding, and the 256-byte `WorkflowId` limit.

TODO/backlog: the older start and repair design in this document described an
SQLite pre-start reservation plus `starting`, `stale_start`, and
`stale_missing` repair. Do not implement that model. `WORKFLOW-ACTIVATION.md`
is the newer Temporal-authoritative contract: start is optimistic, current
Describe evidence is projected after start, and SQLite conflicts are
diagnostics rather than reservations or execution authority.

Slice ownership is explicit:

- #502 implements Temporal start, native Run ID, immediate Describe, and typed
  duplicate execution handling without SQLite authority;
- #503 (`Backlog`) is an older start/Run-ID seed now overlapped by promoted
  #502, not another remaining implementation slice;
- #504 (`Backlog`) owns Describe-backed targeted repair/reconciliation;
- capacity admission is an unowned T2607-03 gap with no live Issue;
- #505 (`Backlog`) owns the minimum real caller and App/backend entry surface.

The remaining slices must consume the #501 activation facts and must not
regenerate episode time, accept a caller-selected target kind, or reconstruct
identity. Tracker state above was verified on 2026-07-28; re-check it before
promoting another slice.

## Full Coordinator Goals And Current Tracking

- Start `IssueWorkflow` only for executable tracker states.
- Enforce at most one active `IssueWorkflow` execution per issue at a time.
- Use a human-readable episode-scoped `workflow_id` as the Temporal Workflow ID.
- Store Temporal's native `run_id` after start for exact execution lookup.
- Use SQLite `workflow_index` as a local diagnostic index and App read model.
- Use Temporal start/idempotency and current Describe as execution facts.
- Repair stale local rows without directly changing tracker business state.
- Keep startup and refresh cheap enough for App use.

## Non-Goals

- No background Symphony daemon.
- No App-owned scheduler.
- No full-time tracker scanner.
- No agent execution inside the Coordinator.
- No direct tracker state write inside the Coordinator.
- No business decision about whether an implementation, review, rework, or
  merge should pass.
- No replacement for `IssueWorkflow` state handling.
- No replacement for `TrackerTransitionActivity`.

## Expected Code Areas

The #501 package shape is intentionally small:

```text
symphony/
  coordinator/
    mod.rs
```

Later slices may split files when real start, repair, capacity, and entrypoint
behavior exists. Do not pre-create empty subsystems.

## Core DTOs

The pure #501 contract is:

```text
CoordinatorActivationRequest {
  issue_ref
  expected_tracker_state?
  expected_tracker_revision?
  episode_time
  source_kind
  source_ref
  audit_reason
}

ObservedTrackerSnapshot {
  state
  revision
}

CoordinatorActivationDecision =
  Static
  | Executable {
      observed state/revision
      Coordinator-derived target_kind
      episode_time
      source_kind/source_ref
      audit_reason
      workflow_id
    }
  | StaleExpectation
```

`target_kind` is deliberately absent from `CoordinatorActivationRequest`.
Static and stale-expectation results do not contain executable activation
facts or a `WorkflowId`.

Start DTOs belong to #502, repair DTOs to #504, capacity DTOs to the unowned
T2607-03 gap, and caller DTOs to #505. When added, they must reuse the existing
identity wrappers:

```text
RepoId
IssueRef
WorkflowId
```

Plain strings are acceptable only inside persistence and Temporal client
calls.

## Request Validation

Activation input requires:

- non-empty repository host, owner, and repository components;
- non-zero issue number;
- explicit UTC episode time with exactly second precision;
- stable source kind plus a non-empty source reference;
- a trimmed, non-empty audit reason of at most 512 UTF-8 bytes;
- a non-empty expected or observed tracker revision whenever present.

The source reference is identity. The audit reason is bounded provenance and is
never embedded in the Workflow ID.

## Executable State Policy

The Coordinator classifies the already-observed tracker state without I/O.

| Observed tracker state | Classification | Derived target kind |
| --- | --- | --- |
| `Todo` | executable | `work` |
| `In Progress` | executable | `work` |
| `Agent Review` | executable | `review` |
| `Rework` | executable | `rework` |
| `Merging` | executable | `merge` |
| `Backlog` | static | none |
| `Need to Clarify` | static | none |
| `Need Human Input` | static | none |
| `Human Review` | static | none |
| `Done` | static | none |

Doctor or reconciliation may perform an operation that later changes the
tracker to an executable state. That does not make `Need Human Input` itself
executable.

If an optional expected state or revision differs from the observation,
`StaleExpectation` takes precedence over static/executable classification.

## Workflow ID Construction

The Coordinator owns `workflow_id` construction with this exact grammar:

```text
issue:<encoded-host>/<encoded-owner>/<encoded-repo>:<issue-number>:pulse:<from-state>-to-<target-kind>:<YYYYMMDDTHHMMSSZ>:<source-kind>-<encoded-source-ref>
```

Rules:

- `workflow_id` is the Temporal Workflow ID.
- Store Temporal's returned `run_id` separately in later start work.
- Tracker state, target kind, and source kind use stable lowercase kebab-case.
- Coordinator derives target kind; callers cannot choose it.
- Repository components and source reference use reversible URL-safe
  percent-encoding of UTF-8 bytes. Preserve unreserved characters and case.
- Episode time is explicit UTC-second input and is never generated inside
  identity construction.
- Identical activation input, including uncertain retries, reuses the exact ID.
- A new episode timestamp or source identity creates a new ID.
- The complete encoded ID is limited to 256 bytes. Overflow is a typed error;
  never truncate, hash, slugify, or regenerate input.

Examples:

```text
issue:github.com/Alive24/shea-symphony:123:pulse:todo-to-work:20260708T134218Z:tracker-project-rev-456
issue:github.com/Alive24/shea-symphony:123:pulse:merging-to-merge:20260708T150012Z:operator-action-action-789
```

## Start Flow

#502 implements the start contract from `WORKFLOW-ACTIVATION.md`:

```text
receive already-observed executable activation facts
  -> build backward-compatible durable IssueWorkflowInput
  -> start Temporal once with the exact existing workflow_id
  -> Describe once by workflow_id and known run_id when available
  -> return independent start evidence and execution observation
```

Start sets `WorkflowIdReusePolicy::RejectDuplicate` and
`WorkflowIdConflictPolicy::Fail`. Typed SDK `AlreadyStarted` evidence remains
distinct from an indeterminate start operation. Accepted starts retain the real
non-empty SDK handle Run ID. Start and Describe are orthogonal so a newly
accepted Workflow that closes before Describe is still represented correctly.
The Coordinator connection disables the SDK operation retry loop; retries occur
only when a caller deliberately resubmits the exact activation.

Do not insert a SQLite `starting` reservation before Temporal start. A local
active-row conflict is diagnostic input for #504 reconciliation, not authority
to reject or authorize a start. Capacity policy remains an unowned prerequisite
for the future caller and is not implicitly implemented inside #502.

## Discovery Triggers

Discovery remains deferred to #505 and should stay explicit and bounded.
Allowed future triggers include tracker refresh, routed operator action, and a
bounded visible/startup repair request. Static states never activate directly.

## Capacity Policy

TODO (unowned T2607-03 gap): define and implement admission against the
configured task-queue policy. Capacity deferral must not mutate tracker state,
create a SQLite reservation, or regenerate activation identity. The next
attempt reuses the same activation facts unless the caller intentionally
creates a new episode.

## Repair Flow

TODO #504: implement targeted reconciliation from Temporal/current Describe
evidence under the newer `WORKFLOW-ACTIVATION.md` and `LOCAL-STATE-DB.md`
projection contract.

The v1 projector does not create `starting`, `stale_start`, or `stale_missing`
rows. Missing, conflicting, or stale local evidence must be reconciled against
Temporal; no SQLite row may reserve a start or authorize a new execution.
Repair does not move tracker business state directly.

## Temporal Interaction

#502 uses Temporal client APIs for one start and one current execution
Describe. The pure #501 activation contract still performs no Temporal I/O.

Start attributes should carry the validated repository/issue identity, observed
tracker state/revision, Coordinator-derived target kind, source kind/reference,
episode time, and audit reason. `IssueWorkflow` runs on `symphony-core`.

`IssueWorkflowInput.started_at` is the RFC 3339 UTC-second activation episode
time, not Temporal's authoritative start time. Describe returns the latter as
`temporal_started_at`.

TODO(T2607-03): Search Attributes/Visibility indexing is not designed or
implemented by #502. Assign it only after #504/#505 establish repair/read-model
and caller boundaries.

## SQLite Interaction

#504 may project current Describe-backed lifecycle evidence through
`LocalStateProjector`. SQLite provides an App read model and diagnostic active
index. It cannot reserve, reject, authorize, or prove a Temporal start.

The pure #501 activation contract performs no SQLite reads or writes.

## Tracker Interaction

#501 accepts an already-observed tracker snapshot and performs no tracker I/O.
#502 also performs no tracker I/O because it consumes the validated #501
activation. #504/#505 may read at their assigned durable boundaries, but
Coordinator must not write tracker state. Tracker writes belong to
`TrackerTransitionActivity`.

## App And Operator Interaction

TODO #505: expose only the minimum real caller after #502 start, #504 repair,
and the unowned capacity contract exist. The pure #501 module is crate-private
and adds no App, Tauri, Svelte, or CLI surface.

## Error Handling

#501 uses typed validation errors for invalid issue/source/revision/audit/time
input and Workflow ID overflow. #502 distinguishes pre-dispatch
input/configuration/payload failures, definitive server rejection,
unavailable/indeterminate side-effect outcomes, and malformed/contradictory
protocol evidence with Connect/Start/Describe attribution. #504, #505, and the
unowned capacity slice own repair, entrypoint, and admission outcomes without
changing this identity policy.

## #501 Acceptance Checks

- All ten tracker states have one explicit classification.
- Executable states have exactly one Coordinator-derived target kind.
- `Need Human Input` is static.
- Matching expectations can produce a static or executable decision.
- Stale expectations produce no executable facts or Workflow ID.
- URL-safe component encoding is reversible and preserves case and UTF-8.
- Identical activation input produces the same retry-stable Workflow ID.
- New episode time or source identity produces a different Workflow ID.
- Complete encoded IDs over 256 bytes fail without fallback identity.
- The module and its contract remain crate-private and reuse `RepoId`,
  `IssueRef`, and `WorkflowId`.
- No tracker, SQLite, Temporal, capacity, filesystem, network, process, CLI,
  App, Svelte, or Tauri side effect exists.

## #501 Done Means

- pure activation request, observation, decision, executable facts, enums, and
  typed errors exist;
- state classification and target derivation are centralized and table-tested;
- Workflow ID grammar, encoding, validation, and retry behavior are centralized
  and tested;
- semantic Rustdoc and boundary comments explain the durable identity policy;
- deferred #502/#504/#505 and unowned capacity work is explicit, #503 is
  recognized as an overlapped Backlog seed, and the older SQLite reservation
  model is not accidentally implemented.

## #502 Acceptance Checks

- Only `CoordinatorExecutableActivation` can enter the start boundary.
- Durable input maps the exact Workflow ID, issue identity, observed state and
  revision, derived target, source provenance, episode time, and audit reason.
- Legacy durable input JSON without `tracker_backend`, `source_kind`, or
  `audit_reason` still deserializes through Serde defaults.
- Each invocation performs at most one start and one immediate Describe.
- Accepted starts return the real SDK Run ID; duplicate starts remain typed.
- Uncertain starts never become "not started" and may converge through Describe.
- Open, Closed, and DescribeRequired observations remain independent of start
  evidence.
- No tracker, SQLite, capacity, Search Attribute, App, or Workflow business
  state path is introduced.
