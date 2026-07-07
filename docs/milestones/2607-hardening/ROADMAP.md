# 2607 Hardening Roadmap

Status: Draft

## Phase 0: Documentation Spine

- Create milestone docs.
- Record the Symphony/Shea boundary.
- Record Workflow Graph direction without making it a 2607 runtime rewrite.
- Capture open questions without promoting them to GitHub issues yet.

## Phase 1: Subtraction First

- Do not add new user-visible features.
- Keep Main, Review, Merge, Doctor, App status reads, and existing workflows
  working.
- Inventory repeated tracker reads and command paths.
- Identify every direct tracker write path.
- Mark which writes must move behind Symphony transition ownership.
- Identify lane-local state mapping that should move to the standard state
  model.
- Identify UI/read-surface paths that infer source-of-truth state.
- Identify runtime and vendored binary assumptions that belong in local install
  and config.
- Split or move files only when the move clarifies ownership and preserves
  behavior.
- Allow docs, tests, instrumentation, internal adapters, timing, read dedupe,
  and small state helpers.

Success means a maintainer can answer:

- Who owns this state transition?
- Which snapshot was used?
- Which Tracker State and workflow step ran?
- Which component was allowed to write?
- Where is the evidence?
- What can be retried safely?

## Phase 2: Snapshot And State

- Define `SymphonySnapshot` for App reads.
- Keep top-level dashboard refresh separate from lane item detail.
- Introduce one Project/Tracker snapshot per runtime tick.
- Make runtime state the source of truth for running, retrying, stalled, and
  terminal worker state.
- Require tracker transition success as part of lane handoff completion.
- Make terminal claim cleanup part of state transition handling.

## Phase 3: Transition Kernel

- Define transition proposal, decision, and commit records.
- Route tracker writes through one Symphony-owned transition service.
- Require transition evidence.
- Add reconcile behavior for external tracker changes.
- Keep extension nodes able to influence graph direction through proposals.

## Phase 4: Runner And Worktree

- Keep agent running inside Symphony.
- Keep worktree creation and ownership inside Symphony.
- Move target repository workspaces under `~/.shea/` by default.
- Stop vendoring Symphony binaries into target repos.

## Phase 5: Workflow Structure Preparation

- Keep current workflow configuration compatible.
- Organize workflow documentation around Tracker State.
- Define standard behavior configuration points.
- Define where extension hooks/nodes may be inserted.
- Defer graph execution and extension module loading to 2608 Workflow Graph
  Extension.

## Phase 6: Shea Extensions

- Reconnect skills, prompt templates, semantic gates, Issue Forge, Dream, and
  operator workflows as extensions over Symphony.
- Keep extension side effects constrained by Symphony runtime policy.

## Phase 7: Performance Hardening

- Measure non-LLM control-plane paths.
- Remove repeated tracker reads.
- Remove UI-triggered command churn.
- Keep slow external dependencies visible in status snapshots.
