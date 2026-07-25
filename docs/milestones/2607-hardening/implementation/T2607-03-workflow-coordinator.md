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

Deferred ownership is explicit:

- #502 owns Temporal start and already-open execution handling;
- #503 owns Describe-backed targeted repair/reconciliation;
- #504 owns capacity admission;
- #505 owns the minimum real caller and App/backend entry surface.

Those slices must consume the #501 activation facts and must not regenerate
episode time, accept a caller-selected target kind, or reconstruct identity.

## Full Coordinator Goals Across #501-#505

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

Start, repair, capacity, and caller DTOs remain deferred to #502-#505. When
added, they must reuse the existing identity wrappers:

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

TODO #502: implement the start contract from
`WORKFLOW-ACTIVATION.md`. The future flow is:

```text
receive already-observed executable activation facts
  -> apply capacity policy owned by #504
  -> start Temporal with the existing workflow_id
  -> Describe the current execution
  -> project only Describe-backed lifecycle evidence
```

Do not insert a SQLite `starting` reservation before Temporal start. If a
Workflow with the same retry-stable ID is already open, establish that through
Temporal and bind/project the described execution. A local active-row conflict
is diagnostic input for #503 reconciliation, not authority to reject or
authorize a start.

## Discovery Triggers

Discovery remains deferred to #505 and should stay explicit and bounded.
Allowed future triggers include tracker refresh, routed operator action, and a
bounded visible/startup repair request. Static states never activate directly.

## Capacity Policy

TODO #504: define and implement admission against the configured task-queue
policy. Capacity deferral must not mutate tracker state, create a SQLite
reservation, or regenerate activation identity. The next attempt reuses the
same activation facts unless the caller intentionally creates a new episode.

## Repair Flow

TODO #503: implement targeted reconciliation from Temporal/current Describe
evidence under the newer `WORKFLOW-ACTIVATION.md` and `LOCAL-STATE-DB.md`
projection contract.

The v1 projector does not create `starting`, `stale_start`, or `stale_missing`
rows. Missing, conflicting, or stale local evidence must be reconciled against
Temporal; no SQLite row may reserve a start or authorize a new execution.
Repair does not move tracker business state directly.

## Temporal Interaction

#502 and #503 will use Temporal client APIs for start and current execution
Describe. The pure #501 activation contract performs no Temporal I/O.

Start attributes should carry the validated repository/issue identity, observed
tracker state/revision, Coordinator-derived target kind, source kind/reference,
episode time, and audit reason. `IssueWorkflow` runs on `symphony-core`.

## SQLite Interaction

#503 may project current Describe-backed lifecycle evidence through
`LocalStateProjector`. SQLite provides an App read model and diagnostic active
index. It cannot reserve, reject, authorize, or prove a Temporal start.

The pure #501 activation contract performs no SQLite reads or writes.

## Tracker Interaction

#501 accepts an already-observed tracker snapshot and performs no tracker I/O.
#502/#503/#505 may read at their durable boundaries, but Coordinator must not
write tracker state. Tracker writes belong to `TrackerTransitionActivity`.

## App And Operator Interaction

TODO #505: expose only the minimum real caller after start, repair, and capacity
contracts exist. The pure #501 module is crate-private and adds no App, Tauri,
Svelte, or CLI surface.

## Error Handling

#501 uses typed validation errors for invalid issue/source/revision/audit/time
input and Workflow ID overflow. #502-#505 own typed I/O, conflict, capacity, and
entrypoint outcomes without changing this identity policy.

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
- deferred #502-#505 work is explicit and the older SQLite reservation model is
  not accidentally implemented.
