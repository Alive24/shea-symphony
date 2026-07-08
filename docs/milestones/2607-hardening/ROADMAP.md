# 2607 Hardening Roadmap

Status: Draft

## Phase 0: Documentation Spine

- Create milestone docs.
- Intake official Temporal Rust SDK materials before locking implementation
  contracts.
- Record the Symphony/Shea boundary.
- Ground the Temporal/Codex/review/Shea role split against current repo
  modules.
- Record Temporal local runtime as the 2607 orchestration spine.
- Record Workflow Graph direction without making it a 2607 runtime rewrite.
- Capture open questions without promoting them to GitHub issues yet.

## Phase 1: Subtraction First

- Do not add new user-visible features.
- Migrate Main, Review, Merge, Doctor, App status reads, and existing workflows
  into Temporal-backed runtime paths.
- Inventory repeated tracker reads and command paths.
- Identify every direct tracker write path and move it toward
  `TrackerTransitionActivity`.
- Identify lane-local state mapping that should move into `IssueWorkflow`.
- Identify UI/read-surface paths that infer source-of-truth state.
- Identify runtime and vendored binary assumptions that belong in local install
  and config.
- Split or move files only when the move clarifies ownership and preserves
  behavior.
- Allow docs, tests, instrumentation, internal adapters, timing, read dedupe,
  and small state helpers.

Success means a maintainer can answer:

- Who owns this state transition?
- Which Temporal query or activity result was used?
- Which Tracker State and workflow step ran?
- Which component was allowed to write?
- Where is the evidence?
- What can be retried safely?

## Phase 2: Temporal Runtime Spine

- Make local Temporal service the 2607 orchestration backend.
- Define `IssueWorkflow` across every standard Shea Symphony state.
- Include `Backlog` promotion and quality gate inside `IssueWorkflow`.
- Mark old autopilot/tick/resume loop as legacy-to-delete.
- Define worker startup and local runtime initialization.
- Keep Temporal Cloud out of scope.

## Phase 3: Query-Backed Snapshot And State

- Define query-backed `SymphonySnapshot` for App reads.
- Keep top-level dashboard refresh separate from lane item detail.
- Introduce Temporal queries for dashboard and issue detail.
- Make Temporal workflow state the durable source for running, retrying,
  waiting, and terminal worker state.
- Require tracker transition success as part of lane handoff completion.
- Make terminal claim cleanup part of state transition handling.

## Phase 4: Transition Activity

- Define transition proposal, decision, and commit records.
- Route tracker writes through `TrackerTransitionActivity`.
- Use small DTOs for Workflow history and keep rich tracker evidence behind
  artifact refs or targeted Activity reads.
- Reuse existing tracker adapter, recovery marker, readback, workpad, and audit
  behavior inside the Activity boundary instead of wrapping the old lane loop.
- Require transition evidence.
- Add reconcile behavior for external tracker changes.
- Define merge-time semantic fix behavior as part of `Merging`.
- Keep extension nodes able to influence graph direction through proposals.
- Complete the transition migration through explicit submilestones: contract,
  state commits, evidence commits, claim/field diet, reconcile/recovery, and
  deletion of old mutation paths.

## Phase 5: Activities, Runner, And Worktree

- Run agent, review, merge, doctor, tracker, and worktree side effects as
  Temporal Activities.
- Prefer coarse Activity boundaries around existing lane/runtime behavior
  instead of modeling every model turn or tool call as a workflow step.
- Keep worktree creation and ownership inside Activities owned by Symphony.
- Move target repository workspaces under `~/.shea/` by default.
- Stop vendoring Symphony binaries into target repos.

## Phase 6: Workflow Structure Preparation

- Keep current workflow configuration compatible.
- Organize workflow documentation around Tracker State.
- Define standard behavior configuration points.
- Define where extension hooks/nodes may be inserted.
- Defer graph execution and extension module loading to 2608 Workflow Graph
  Extension.

## Phase 7: Shea Extensions

- Reconnect skills, prompt templates, semantic gates, Issue Forge, Dream, and
  operator workflows as extensions over Symphony.
- Keep extension side effects constrained by Symphony runtime policy.

## Phase 8: Performance Hardening

- Measure non-LLM control-plane paths.
- Remove repeated tracker reads.
- Remove UI-triggered command churn by using Temporal queries/signals/updates.
- Keep slow external dependencies visible in status snapshots.
