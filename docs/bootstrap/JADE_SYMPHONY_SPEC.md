# Jade Symphony Specification

Status: Bootstrap v0

## Purpose

Jade Symphony is a private-first team harness for orchestrating coding agents
against tracked engineering work. It is a Rust implementation of an
OpenAI-Symphony-style orchestration system with Jade Symphony-specific workflow
extensions.

This document is an extension spec. It does not replace the official OpenAI
Symphony specification or the Elixir reference implementation.

## Baseline Rule

Jade Symphony should preserve the capability categories present in the official
OpenAI Symphony reference unless a divergence is explicitly recorded.

Source order:

1. `docs/bootstrap/references/openai-symphony/SPEC.md`
2. `docs/bootstrap/references/openai-symphony/elixir/README.md`
3. `docs/bootstrap/references/openai-symphony/elixir/WORKFLOW.md`
4. `docs/bootstrap/references/openai-symphony/elixir/lib/`
5. Jade Symphony-specific bootstrap docs in `docs/bootstrap/`

`SPEC.md` is the normative protocol baseline. The Elixir implementation is the
feature-parity baseline. Jade Symphony may use different Rust architecture, but
it should not silently drop Elixir capabilities.

If a reference capability is delayed, record it in a parity roadmap with:

- capability name.
- reference source path.
- implementation status.
- reason for delay.
- planned implementation path.

## Official Capability Categories To Preserve

The implementation should account for these official Symphony categories:

- `WORKFLOW.md` loading with YAML front matter plus prompt body.
- typed runtime configuration and validation.
- tracker client abstraction and normalized issue records.
- active, terminal, and non-active state handling.
- polling, dispatch, retry, stop, release, and reconciliation behavior.
- startup and reconciliation cleanup for terminal workspaces.
- per-issue workspace lifecycle and hooks.
- prompt rendering from issue context and workflow template.
- agent runner abstraction.
- Codex app-server backend behavior.
- structured logs.
- runtime snapshot.
- terminal/operator-readable status surface.
- optional web/API observability capability category.
- path safety and workspace boundary controls.
- conformance-oriented tests.

Jade Symphony does not need to copy Elixir module structure or Phoenix
implementation details. It does need to preserve the operational capability
unless the parity roadmap records a deliberate difference.

## Jade Symphony Extensions

Jade Symphony adds these workflow capabilities on top of the official baseline:

- GitHub Project v2 tracker adapter as the first concrete tracker.
- Linear tracker adapter as a required supported adapter, even if implemented
  after GitHub Project v2.
- assignee filtering for multi-owner dispatch.
- Issue Forge for proposing, drafting, validating, and repairing executable
  issues through a question-driven clarification loop.
- Issue Quality Gate before agent dispatch.
- additional normalized states:
  - `Need to Clarify`
  - `Need Human Input`
  - `Agent Review`
- independent agent review before human review.
- Claude Code support through the same backend abstraction as Codex.

## Runtime Autonomy

Jade Symphony should operate as a continuing tracker loop, not a one-issue
command runner. A human "continue" instruction means:

1. refresh tracker and worktree state.
2. finish active unblocked `In Progress` work.
3. otherwise select the next executable `Todo` or `Rework` item.
4. run the Issue Quality Gate.
5. execute the issue contract.
6. move main-agent completed work to `Agent Review`.
7. repeat until a defined stop condition is reached.

Stop conditions are limited to:

- no executable work remains.
- issue contract requires `Need to Clarify`.
- implementation requires `Need Human Input`.
- required external credentials, services, sample data, or tools are missing and
  no safe fallback exists.
- verification fails and cannot be locally repaired.
- continuing requires destructive action or out-of-contract scope change.
- the human explicitly asks to stop after a specific issue.

Dependency wording such as "do not continue to issue X until issue Y is done"
is not a permanent stop condition. After issue Y reaches its correct handoff or
blocked state, the runtime should resume selection.

## Product Boundary

Jade Symphony is orchestration infrastructure. It should not contain downstream
application business logic.

This boundary does not mean the harness should be artificially reduced. It means
domain-specific work belongs in tracked issues and repository workspaces, not in
the orchestration core.

## Architecture Layers

Keep these layers separate:

1. `workflow`: load workflow front matter and prompt body.
2. `config`: validate typed runtime settings.
3. `tracker`: normalize tracker-specific work items into one domain model.
4. `issue_forge`: propose, draft, validate, and repair executable issue
   contracts.
5. `quality_gate`: decide whether a tracked issue is dispatchable.
6. `orchestrator`: poll, dispatch, retry, stop, release, and reconcile.
7. `workspace`: create, reuse, and clean per-issue workspaces.
8. `agent`: render prompt and run Codex or Claude Code backends.
9. `review`: run independent agent review before human review.
10. `event_log`: write structured logs and final run summaries.
11. `status_surface`: expose operator-readable runtime state.
12. `observability_api`: optional web/API observability compatible with the
    reference capability category.

## Tracker Model

The orchestrator should not depend on GitHub or Linear objects directly. It
should consume normalized tracker records.

```text
TrackerIssue
  tracker_kind
  id
  item_id
  identifier
  title
  description
  url
  state
  labels
  assignees
  priority
  branch_name
  linked_pull_requests
  blocked_by
  project_fields
  created_at
  updated_at
```

For GitHub Project v2:

- `id` is the GitHub Issue node ID.
- `item_id` is the ProjectV2 item ID.
- `identifier` is `#<issue number>`.
- `state` comes from the ProjectV2 `Status` field.
- `description` comes from the GitHub Issue body.
- assignees, labels, comments, and linked PRs come from the GitHub Issue.

For Linear:

- `id` is the Linear issue ID.
- `item_id` may be null or equal to `id`.
- `identifier` is the Linear issue key.
- `state` comes from Linear issue state.
- `description` comes from Linear issue description.

## Normalized States

Jade Symphony should normalize tracker states to:

- `Backlog`: out of scope for dispatch.
- `Todo`: queued and eligible for dispatch after gate checks.
- `Need to Clarify`: issue contract is not executable yet.
- `In Progress`: implementation is actively underway.
- `Need Human Input`: work started but cannot continue without human input.
- `Agent Review`: main-agent implementation is locally complete and awaiting
  independent Review Agent execution.
- `Human Review`: independent Review Agent has passed the work with recorded
  evidence; waiting on human approval.
- `Rework`: reviewer requested changes.
- `Merging`: approved by human; land flow should run.
- `Done`: terminal success.

Tracker adapters map their native state fields to this normalized set. GitHub
Project v2 maps ProjectV2 `Status` options. Linear maps issue workflow states.

## Issue Forge

Issue Forge is the upstream issue-creation and issue-repair capability. It
should not invent issues from thin air. It should synthesize candidate issues
from current repository reality, canonical docs, recent development progress,
existing tracker state, and human intent.

Issue Forge should use a question-driven clarification loop when intent is
underspecified. The loop should ask one focused question at a time, incorporate
the answer into the candidate issue contract, and continue only while the answer
materially changes scope, acceptance criteria, guardrails, or validation. It
should stop when the issue is executable, not when every possible detail has
been discussed.

Issue Forge modes:

- `discover`: compare canonical intent, current implementation, recent changes,
  and existing tracker work to propose candidate issues.
- `discuss`: run the clarification loop for one candidate.
- `draft`: produce a tracker-ready issue using the issue quality template.
- `validate`: inspect an existing issue for executability, duplication, scope,
  and source alignment.
- `repair`: update or propose changes to an underspecified issue.

Clarification loop rules:

- Ask at most one primary question per turn unless multiple questions are
  tightly coupled.
- Prefer concrete tradeoff questions over broad brainstorming prompts.
- Preserve human answers as decisions or assumptions in the issue draft.
- Separate execution-critical unknowns from nice-to-have detail.
- Route missing execution-critical information to `Need to Clarify`.
- Turn scope expansion into follow-up candidates instead of bloating the current
  issue.
- Stop asking once the issue can pass the Issue Quality Gate.

Candidate classifications:

- `Ready`
- `Ready With Assumptions`
- `Need to Clarify`
- `Too Broad`
- `Blocked`
- `Duplicate / Already Covered`

Issue Forge should use `docs/bootstrap/ISSUE_QUALITY_GATE_TEMPLATE.md` as the
contract shape.

## Issue Quality Gate

Before dispatch, an issue is executable only if it has enough information for
an agent to act without inventing product, architecture, or operational intent.

Required issue contract:

- goal.
- why now.
- scope.
- non-goals.
- target repo or package.
- target surfaces or files when known.
- canonical references.
- acceptance criteria.
- validation requirements.
- guardrails and constraints.
- required credentials, sample data, or external dependencies.

If the contract is missing critical context:

1. create or update the workpad.
2. record the missing information.
3. move the issue to `Need to Clarify`.
4. ask for the smallest useful clarification.

If the issue is executable with assumptions:

1. record assumptions in the workpad.
2. continue through normal dispatch.

## Agent Backends

Backends must consume the same rendered issue prompt and return normalized run
events and final results.

```text
AgentBackend
  name
  prepare(workspace, rendered_prompt, config)
  run()
  stream_events()
  stop(reason)
  summarize()
```

Codex should be the first concrete backend. Claude Code should be implemented
as a peer backend, not as a special case inside the orchestrator.

## Agent Review

`Agent Review` is a first-class state between implementation and human review.

Jade Symphony must distinguish the main implementation agent from the
independent Review Agent.

The main implementation agent may move locally complete work to `Agent Review`.
It must never move an issue to `Human Review`.

The Review Agent may be Gemini, Codex, Claude, or another configured reviewer.
The first concrete Review Agent backend should use local Gemini CLI when
available. Review execution must be asynchronous: launching review must not
block the main orchestrator loop, and review jobs must be externally observable
while queued, running, completed, failed, timed out, or cancelled.

The orchestrator should treat reviewer output as advisory evidence, not as final
truth.

Review findings should be classified as:

- `Confirmed`
- `Plausible`
- `Rejected`
- `Needs Context`

Review Agent state transitions:

- passed review with recorded evidence: ordinary issues and parent final issues
  move from `Agent Review` to `Human Review`; routine native subissues move to
  `Merging` because the parent issue owns final Human Review and UAT.
- confirmed findings: move to `Rework`.
- failed, timed out, inconclusive, or unavailable review backend: keep in
  `Agent Review` or move to `Need Human Input`.

Confirmed findings should be fixed before `Human Review`. Rejected or deferred
findings should be recorded in the workpad.

## Observability And Status

Jade Symphony should preserve the official observability capability category.

Required:

- structured event log.
- run summary.
- runtime snapshot.
- operator-readable status surface.

Expected future-compatible shape:

- terminal status view.
- status command.
- optional web/API observability surface.

The exact UI technology is implementation-defined. Do not remove observability
from scope merely because the first implementation is a CLI.
