---
name: shea-symphony-issue-forge
description: Shape rough operator intent into a quality-gated Shea Symphony GitHub issue through focused discussion, explicit confirmation, and guarded workflow creation or rework.
---

# Shea Symphony Issue Forge

Use this skill to turn a rough idea into a dispatchable Shea Symphony issue. Conversation and drafting happen here; deterministic validation and tracker writes happen only through the configured Forge workflow surface.

## Bind the target

Resolve the target repository, workflow configuration, tracker project, default assignee, and supported quality-gate/create/rework actions. Do not assume a repository slug, local path, workflow filename, account, or command. Ask for a missing binding before making a tracker write.

## Discuss before creating

Start from the operator's intent and ask only one to three focused questions per turn. Ask another short round only while an ambiguity would change execution. Act as a thinking partner rather than a form:

- offer a recommended answer where the operator has implied one;
- keep low-level implementation detail out unless it changes the contract;
- state that the operator can skip discussion and proceed with recorded assumptions;
- keep deferred ideas separate instead of inflating the issue.

Resolve goal, why now, target package, in/out of scope, guardrails, dependencies, trusted references, current-state freshness, verification, and operator-facing UAT. Check whether a native parent/subissue batch is warranted when the work has independently testable slices, multiple lanes, high review risk, or would otherwise create an oversized PR.

For a batch, the parent owns final Human Review and UAT. Each ordinary child is an implementation slice with independent Agent Review but no routine direct Human Review or UAT. Record a Subissue Human Review Exception only when a child truly needs it.

Never create dispatchable Todo work that depends on a blocker represented only in prose. Create the structured relationship in the same workflow action, or keep the work in Backlog until the blocker is terminal.

## Draft contract

Use this complete shape, omitting only genuinely inapplicable optional reference sections:

```
## Issue Setup

- UAT Required: Yes / No
- Assignee: <resolved default>
- Dependencies: None / <structured blocker>
- Related Parent Issue or Context: <link or None>

## Issue Goal

<one concrete outcome>

## Issue Context

### Why Now

<why this matters now>

### Target Repository / Package

- <resolved target>

## Non-Negotiable Guardrails

- <must not change / safety boundary>

## Scope

### In Scope

- <deliverable>

### Out of Scope

- <explicit exclusion>

## Canonical References

### Relevant Knowledge Sources

- <local path or needed external URL>

### Relevant Code Paths

- <path>

## Current State

<what was checked and when>

### Code-State Freshness

<main, relevant PRs, and known drift>

## Deliverable Shape

<observable result>

## Risks or Constraints

- <risk or None>

## Expected Outcome

- [ ] <objective result>

## Verification

### Completion Criteria

- [ ] <objective completion condition>

### Functional Verification

- [ ] <project-supported command or observable test>

### UAT

- [ ] <operator-owned acceptance action, or Not required>

### Context Verification

- [ ] Confirm the contract still matches current main, relevant PRs, and
      recently completed work before dispatch.
```

All checklist statements must be independently checkable from a diff, workpad, timeline evidence, command result, or operator evidence. Keep local reference paths unadorned in Relevant Knowledge Sources so the quality gate can resolve them.

## Create, promote, or rework

Show the complete draft and obtain explicit confirmation before creating or promoting. Then:

1. Write the approved body to a workflow-approved temporary location.
2. Run the configured Forge quality gate.
3. Repair only the named omissions when it returns NeedToClarify, then rerun.
4. Use the guarded create or promotion action with explicit write authority.
5. Read the created or changed issue back through the workflow or provider.

For a live Human Review issue whose execution contract must change, do not use raw Project mutation or ordinary promotion. Discuss the revised contract, prepare a full replacement body and evidence, obtain confirmation, then invoke the guarded Forge Rework action.

The creation or status-routing operation is the final mutation. Report the issue URL, number, Project status, assumptions, and any dogfood finding after readback.

## Safety

- Never create an issue without explicit confirmation unless the operator has directly instructed creation from a complete body.
- Never bypass the quality gate with raw tracker creation.
- Do not modify implementation code in this skill.
- Stop rather than duplicate an issue when tracker reads are unavailable.
