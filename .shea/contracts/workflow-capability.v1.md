---
kind: shea-workflow-capability
contract_version: 1
active_workflow: ../workflows/shea-symphony.md
adapters:
  - id: legacy-cli-v1
    path: adapters/legacy-cli.v1.md
---

# Workflow Capability

This contract gives Shea skills one target-neutral vocabulary for reading and
acting on an active workflow. It is an authority and navigation surface, not a
permission grant or a command runbook.

## Ownership

- The active workflow owns repository, tracker, state, lane, workspace,
  verification, backend policy, and repository Markdown paths for lane prompts,
  the executable-Issue template, and workpad templates.
- Machine-local profiles own executable paths and environment requirements.
- An adapter maps these semantics to a runtime surface and reports which
  capabilities it supports.
- A consuming skill owns its lane or operator policy and may use only the
  subset allowed by that policy.

Consumers resolve `active_workflow`, select one listed adapter, and fail closed
when either reference is missing, stale, or unsupported. They do not copy
resolved workflow or profile values into this contract.

## Targeted Reads

- `workflow.resolve`: resolve the active workflow and selected adapter.
- `issue.read`: read one issue's normalized state, fields, and issue contract.
- `issue.inspect`: read one issue's lane-specific eligibility and quality gate.
- `evidence.read`: read one issue's canonical workpad and append-only evidence.
- `pull_request.read`: read one pull request and its exact issue-link source.
- `relationships.read`: read blockers, parent, and native subissues for one issue.

Prefer the narrowest read that answers the current question. A Project-wide
scan is not a substitute for a targeted read.

## Guarded Actions

- `workspace.adopt`: record the selected canonical issue workspace.
- `lane.claim`: record one lane's ownership after eligibility and readiness.
- `workpad.upsert`: merge named stable sections into the one canonical Main
  workpad without erasing unrelated evidence; append-only lane records remain
  separate.
- `timeline.append`: add immutable evidence outside the canonical workpad.
- `issue.transition`: request one allowed workflow state transition.
- `pull_request.link`: record or verify the issue's pull request relationship.
- `relationship.add_blocked_by`: add one native blocker relationship.
- `relationship.add_subissue`: add one native parent/subissue relationship.
- `issue.create`: create one validated issue contract.
- `issue.promote`: replace one Backlog seed with a validated executable contract.
- `issue.rework`: replace one Human Review contract with confirmed Rework scope.

These names describe intent. The selected adapter owns implementation syntax;
the active workflow and consumer policy still decide whether an action is
available in the current state.

## Mutation Protocol

Every guarded action follows the same four phases:

1. **Prepare** — perform targeted reads, validate authority and preconditions,
   and render the exact intended effect without writing.
2. **Confirm** — obtain an explicit confirmation bound to that prepared effect.
   A supervised lane may carry pre-authorized confirmation only when its launch
   contract names the exact issue, lane, and allowed action.
3. **Execute** — invoke the selected adapter once with the confirmed inputs.
4. **Targeted readback** — reread the affected issue, evidence, relationship, or
   pull request and compare the observed effect with the prepared effect.

Do not broaden a confirmation, infer it from a read-only request, or treat
adapter success output as readback. When a phase changes workflow state, state
is the final mutation after its supporting evidence is durable.

## Failure And Uncertain Writes

- A failure before Execute is `not_applied`; repair or prepare again.
- An authoritative rejection is `rejected`; preserve the reason and stop.
- A timeout, interrupted connection, or malformed result after Execute is
  `uncertain`; do not retry blindly.
- For `uncertain`, perform the targeted readback once and classify the effect as
  `applied`, `not_applied`, or `ambiguous`.
- Continue after `applied`. Re-prepare before retrying `not_applied`.
- Stop for recovery or human direction on `ambiguous`, conflicting ownership,
  stale confirmation, or changed preconditions.

Adapters may document idempotent recovery mechanics, but cannot weaken these
classification, confirmation, or readback rules.

## Markdown Source Authority

Repository Markdown owns operator- and agent-facing prose for lane prompts and
workpads. Runtime code owns selection, typed interpolation values, rendering,
section-aware idempotent merge mechanics, validation, and tracker transport.
When an active workflow declares Markdown templates, missing, empty, unreadable,
or malformed required files make that workflow unready; runtime code must not
silently substitute an embedded prose copy.

The workflow-selected `issue_templates.executable` file is the single owner of
both executable-Issue layout and same-file semantic validation intent. Forge
generation/repair and the optional semantic gate consume that exact raw source;
Rust retains generic render/input safety and tracker/runtime facts only.
