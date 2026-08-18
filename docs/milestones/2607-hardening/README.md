# 2607 Hardening

Status: Draft

## Purpose

2607 Hardening is the post-MVP milestone for turning Shea Symphony from a
working dogfood harness into a maintainable local Temporal workflow runtime.

The MVP proved that the system can move real issues through implementation,
review, merge, and operator follow-up. This milestone focuses on hardening the
runtime boundaries before adding more product surface area.

Workflow Graph implementation is intentionally deferred to
`docs/milestones/2608-workflow-graph-extension/`. 2607 should prepare the
structure and boundaries while replacing the hand-rolled orchestration loop
with Temporal.

## MVP Baseline

The protected 2606 branch is more than historical reference while 2607 is
incomplete. It preserves a complete working Shea Symphony workflow, and App and
CLI binaries built from that protected branch are the active bootstrap
toolchain used to orchestrate development against the current canonical `main`
checkout.

Keep its three roles distinct:

- active development bootstrap while current `main` does not yet provide the
  complete product runtime;
- protected recovery baseline for the self-hosting workflow;
- behavior, test, and operational-evidence oracle for 2607 acceptance.

This operational dependency is not the 2607 architecture. 2607 and later do not
carry the overall 2606 runtime forward as an embedded or wrapped compatibility
layer. Deliberate reuse or extraction of bounded Rust types, parsers, adapters,
helpers, and tests is allowed when ownership is reviewed and the code fits the
new typed boundaries. Required lifecycle and safety semantics move to Temporal,
Activity, Coordinator, Tauri, SQLite, and operator-action contracts; the
vendored App/CLI bootstrap, Autoloop, lane/runtime ownership, and product CLI
command graph are retired.

Issue #534 establishes one bounded transition exception: `main` also builds a
versioned `shea-symphony-legacy` executable for the current App's allowlisted
operator commands. It reuses the already-present legacy CLI modules but is a
separate composition root from the default `shea-symphony` Temporal worker.
The App bundles and validates this sidecar; Temporal never invokes it. This
exception replaces day-to-day App dependence on a protected-branch build and
does not authorize new product behavior in the legacy command graph.

Stop producing new protected-2606 bootstrap builds once the `main` Legacy
sidecar and App bundle have passed release validation, the dogfood App has
migrated to that bundle, and no 2606-only operational blocker remains. Keep the
protected branch as a recovery and behavior oracle until the Temporal product
path covers the operator surfaces and the remaining bootstrap-retirement work
is closed. Delete the Legacy sidecar when those surfaces no longer call the old
command graph and documented recovery no longer requires it.

## Milestone Goal

Separate the reliable runtime named `Symphony` from the extension layer named
`Shea`, while preserving the workflows that already work.

Symphony should own hard execution concerns through Temporal workflows and
activities: tracker reads and writes, workflow state, worktrees, agent running,
review, merge, runtime state, logging, traceability, and status snapshots.
SQLite backs the local read model/cache/index for dashboard and artifact lookup;
it does not own workflow progression.

Shea should own extension concerns: skills, prompt templates, semantic gates,
Issue Forge, Dream/Reflect style backlog mining, and operator interaction.

## Subtraction First

2607 Hardening starts with subtraction. Do not add new user-visible product
capability until the existing runtime boundaries are clearer, repeated
control-plane work is reduced, and tracker/workflow state ownership is
explicit.

Subtraction means:

- remove the hand-rolled durable loop in favor of Temporal local workflows;
- remove duplicate tracker reads inside one workflow step;
- remove scattered tracker writes from lane-specific code;
- remove lane-local state mapping;
- remove App-owned source-of-truth interpretation;
- remove vendored runtime assumptions from target repositories;
- remove hidden state transitions not represented in tracker state, runtime
  state, or workflow evidence;
- remove direct LLM authority over hard runtime decisions.

Subtraction does not mean:

- removing working Main, Review, or Merge flows;
- removing human-in-the-loop states;
- removing Shea extension capabilities;
- discarding the behavior, tests, or operational evidence that define acceptance.

Preserving those semantics does not authorize embedding or wrapping the overall
2606 runtime beneath new interfaces. Old orchestration code may remain
temporarily as inactive reference during migration. Bounded components may be
reused only after explicit ownership review, extraction from legacy
orchestration, and coverage at the new typed boundary.

## Success Criteria

- Symphony has one documented owner for tracker writes.
- Temporal local workflow is the runtime spine for issue orchestration.
- Workflow behavior has a clear state-grouped structure that can evolve toward
  a persistent Workflow Graph without breaking MVP behavior.
- Standard tracker states are explicit and resumable.
- Hooks/extensions can be inserted without gaining direct tracker write access.
- App surfaces consume status and workflow snapshots instead of owning source
  of truth state.
- Workspace and config layout no longer require vendoring the Symphony binary
  into target repositories.
- Non-LLM paths have visible performance expectations and measurement points.

## Non-Goals

- Rewriting the working MVP.
- Reimplementing the Elixir reference one-to-one.
- Adding new user-visible product capability during the subtraction phase.
- Depending on Temporal Cloud.
- Keeping the old autopilot/tick loop as a second durable runtime.
- Creating GitHub issues from every backlog note in this milestone directory.
- Building a full visual Workflow Graph editor.
- Implementing the full Workflow Graph runtime or extension module loader.
- Letting LLM nodes perform unbounded side effects.

## Workstreams

- Boundary definition: decide what belongs to Symphony and what belongs to Shea.
- Temporal spine: migrate issue orchestration to local Temporal workflows and
  activities.
- Workflow structure: define state grouping, standard behavior configuration,
  extension insertion points, and future Workflow Graph vocabulary.
- Tracker ownership: route writes through Symphony.
- Tracker transitions: route state writes through `TrackerTransitionActivity`.
- Workspace/config layout: replace vendored runtime assumptions.
- App/CLI split: make App the primary operation surface and CLI an admin/dev
  fallback.
- Snapshot/dashboard: keep top-level App refresh light and move issue details
  behind drill-down.
- Local state DB: use SQLite as a rebuildable dashboard read model, tracker
  cache, PR summary cache, and artifact index.
- Agent Activity contract: keep coding/review/merge/doctor agent work as
  coarse attempt boundaries with typed inputs, outputs, capabilities,
  heartbeat layers, and artifact refs.
- Child Workflow policy: keep `IssueWorkflow` plus coarse Activities as the
  default; promote subflows to Child Workflows only when they need independent
  durable orchestration.
- Performance: measure and reduce repeated non-LLM control-plane work.
- Implementation backlog: split the hardening work into executable packages
  without prematurely creating GitHub issues.

## Source Documents

- OpenAI Symphony `SPEC.md` pinned at commit `58cf97da06d556c019ccea20c67f4f77da124bf3`: `https://github.com/openai/symphony/blob/58cf97da06d556c019ccea20c67f4f77da124bf3/SPEC.md`
- `docs/bootstrap/SHEA_SYMPHONY_SPEC.md`
- `docs/codex-app-server-transport.md`
- `docs/README.md`
- `docs/legacy-runtime-distribution.md`
- `AGENT-ACTIVITY-CONTRACT.md`
- `CHILD-WORKFLOW-POLICY.md`
- `IMPLEMENTATION-BACKLOG.md`
- `TRACKER-ORGANIZATION.md`
- `FEEDBACK-INTAKE.md`
- `implementation/README.md`
- `implementation/T2607-01-temporal-runtime-skeleton.md`
- `implementation/T2607-02-local-state-db.md`
- `implementation/T2607-03-workflow-coordinator.md`
- `implementation/T2607-04-tracker-transition-activity.md`
- `implementation/T2607-05-agent-activity-boundary.md`
- `implementation/T2607-06-issue-workflow-state-machine.md`
- `implementation/T2607-07-app-integration.md`
- `implementation/T2607-08-deletion-performance-hardening.md`
- `docs/milestones/2607-hardening/RUNTIME-ROLE-MAPPING.md`
- `docs/milestones/2607-hardening/TEMPORAL-RUST-SDK-INTAKE.md`
- `docs/milestones/2607-hardening/TEMPORAL-NOOP-SMOKE.md`
- `docs/milestones/2607-hardening/ACTIVITY-ERROR-TAXONOMY.md`
- `docs/milestones/2607-hardening/ISSUE-WORKFLOW.md`
- `docs/milestones/2607-hardening/ISSUE-WORKFLOW-STATE.md`
- `docs/milestones/2607-hardening/LOCAL-STATE-DB.md`
- `docs/milestones/2607-hardening/OPERATOR-ACTION-BRIDGE.md`
- `docs/milestones/2607-hardening/TASK-QUEUES.md`
- `docs/milestones/2607-hardening/TEMPORAL-CONCURRENCY.md`
- `docs/milestones/2607-hardening/TEMPORAL-SPINE.md`
- `docs/milestones/2607-hardening/WORKFLOW-ACTIVATION.md`
- `docs/milestones/2607-hardening/SUBTRACTION-INVENTORY.md`
- `docs/milestones/2607-hardening/SNAPSHOT-AND-DASHBOARD.md`
- `docs/milestones/2607-hardening/TRACKER-TRANSITIONS.md`
- `docs/milestones/2607-hardening/TRACKER-TRANSITION-ACTIVITY.md`
- `docs/milestones/2607-hardening/adr/0006-temporal-local-runtime-spine.md`
- `docs/milestones/2607-hardening/adr/0007-local-state-db-read-model.md`
- `docs/milestones/2608-workflow-graph-extension/README.md`
- `docs/legacy-runtime-distribution.md`

## Progress Snapshot

GitHub Project #9 owns live execution state. `STATUS.md` is a dated, derived
snapshot for coding agents that need milestone context without reconstructing
it from historical documents. Reconcile the snapshot from the Project and
accepted implementation evidence; do not treat package or ADR status as live
delivery status.

## Open Questions

See `QUESTIONS.md`.

## Decisions

Decision records live under `adr/`.

## Backlog Notes

Backlog notes live under `backlog/`. They are idea capture only until promoted
through the normal issue workflow.
