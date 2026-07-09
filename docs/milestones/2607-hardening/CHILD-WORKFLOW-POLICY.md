# Child Workflow Policy

Status: Draft

## Purpose

Define when 2607 should use Temporal Child Workflows.

Child Workflows are allowed but not the default. `IssueWorkflow` owns the
executable pulse. Activities own side effects and agent attempts.

## Default Rule

2607 should not default core lanes to Child Workflows.

Keep these as Activities unless measured complexity proves otherwise:

- `MainAgentActivity`
- `ReworkActivity`
- `AgentReviewActivity`
- `MergeActivity`
- `TrackerTransitionActivity`
- PR-to-issue link mutation
- artifact write/index
- SQLite projection
- worktree lease acquire/release
- operator action validation

These are issue-pulse attempts or side effects. Making each one a Child
Workflow would add orchestration weight and risk recreating a scheduler beside
`IssueWorkflow`.

## Promotion Criteria

Promote a subflow to Child Workflow only when it has independent durable
orchestration needs:

- its own state machine;
- long waits for multiple external events;
- independent Query, Signal, or Cancel needs;
- complex multi-Activity retry or repair;
- internals the parent should not know;
- failure/cancel semantics that summarize cleanly into a parent result.

## Candidate Child Workflows

### LandWorkflow

Candidate only if land/merge becomes a multi-stage durable flow:

```text
semantic fix -> checks -> merge queue -> post-merge verify -> cleanup
```

If 2607 only runs a land runner and readback verification, keep it as
`MergeActivity`.

### DoctorWorkflow

Candidate only if doctor becomes multi-step repair orchestration:

```text
detect stale index -> query Temporal -> rebuild SQLite -> verify tracker
  -> retry idempotent mutation
```

Small bounded repairs stay as `DoctorActivity` or local/admin Activities.

### ReviewAutofixWorkflow

Candidate only if safe review autofix becomes a loop:

```text
review -> safe fix -> validation -> review again -> verdict
```

2607 can keep this inside `AgentReviewActivity` while results remain bounded
and typed.

### LocalRebuildWorkflow

Candidate for whole-local-state rebuild work such as SQLite projection,
artifact index, or tracker cache rebuilds.

This should be an admin/local workflow, not a Child Workflow under one
`IssueWorkflow`.

## 2607 Decision

2607 first implementation should use `IssueWorkflow` plus coarse Activities.
Child Workflows are a later promotion path for subflows with real independent
durability needs.
