# Workflow Activation

Status: Draft

## Purpose

Define when Symphony starts or resumes an `IssueWorkflow` execution.

Tracker is the durable queue and external workflow state. `IssueWorkflow` is
an executable orchestration episode, not a live execution for every
Shea-managed issue from `Backlog` through `Done`.

## Core Model

Use at most one active `IssueWorkflow` execution per issue at a time.

Between executable episodes, the tracker lane is the durable queue. Static
issues do not need live Workflow executions.

```text
Tracker lane transition creates executable condition
  -> Symphony reconciler/App start command observes executable state
  -> Symphony starts or resumes one IssueWorkflow episode for that issue
  -> Workflow performs work and commits next tracker state
  -> Workflow continues while executable work remains, otherwise completes
     and hands back to tracker/static lane
```

Workflow population is based on executable tracker states, not all
Shea-managed issues in the tracker.

## Static Lanes

These lanes are tracker queues by default and do not automatically keep an
active `IssueWorkflow` execution open:

- `Backlog`;
- `Human Review`;
- `Need Human Input`, unless automatic doctor/reconcile work is required;
- `Done`.

Static does not mean unmanaged. It means the tracker is holding the durable
state until an executable condition appears.

## Executable States

These states can activate or resume an `IssueWorkflow` episode:

- `Todo`: contract check and implementation entry;
- `In Progress`: Main agent work;
- `Agent Review`: agent review work;
- `Rework`: rework agent work;
- `Merging`: land/merge flow;
- `Need Human Input`: only when automatic doctor/reconcile work is required.

`Backlog` promotion to `Todo` creates an executable condition. `Todo` is the
workflow activation point.

## Human Review And Operator Actions

`Human Review` is a static human lane by default.

When a human approves, requests rework, or applies a small fix, the App routes
to Codex/operator flow. The routed flow submits a structured action through the
Operator Action Bridge. Symphony validates the action and starts the
appropriate executable episode:

- approval or human fix can activate validation and `Merging`;
- requested changes can activate `Rework`;
- unresolved ambiguity can move to `Need Human Input`.

The App does not keep a live Workflow open merely because an issue is waiting
for a human.

## Reconciler

The Symphony reconciler or App start command should:

- read tracker states;
- identify executable states;
- start or resume `IssueWorkflow` episodes only for executable states;
- enforce at most one active execution per issue;
- respect task queue and Activity concurrency limits;
- ignore static issues except for dashboard/read-model projection.

Task queue concurrency controls active work, not the number of static issues in
the tracker.

## Episode Completion

An `IssueWorkflow` episode should complete after it commits a static tracker
state and required read-model projection. It may continue only while the issue
remains in executable states.

Examples:

- `Todo` contract check passes and starts implementation, continuing into the
  executable work episode;
- `In Progress` completes PR/link/handoff work and commits `Agent Review`;
- `Agent Review` passes and commits `Human Review`, then completes because
  `Human Review` is static;
- `Merging` lands work and commits `Done`, then completes;
- an operational blocker commits `Need Human Input`, then completes unless an
  automatic doctor/reconcile episode is required.

## Non-Goals

- No live Workflow execution for every tracker issue.
- No long-lived idle Workflow for `Backlog`.
- No long-lived idle Workflow for `Human Review`.
- No App-side scheduler.
- No custom autopilot queue beside tracker plus Temporal.
