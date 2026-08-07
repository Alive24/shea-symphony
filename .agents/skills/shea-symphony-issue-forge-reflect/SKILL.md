---
name: shea-symphony-issue-forge-reflect
description: Review current Shea Symphony progress, issue/PR/run evidence, blockers, and next actions when the operator asks for a status checkpoint, what can proceed, or what remains; extract evidence-backed Backlog seeds or promote a selected Backlog item into a quality-gated Todo contract. Route only concrete failures to shea-symphony-doctor.
---

# Shea Symphony Issue Forge Reflect

Use this skill to turn recent conversations, dogfood findings, Project state, and work records into a manageable Backlog, then help promote one selected Backlog seed into executable work.

Use Reflect for routine operator checkpoints: current progress, what can
proceed next, what remains blocked, and which residual work belongs in Backlog.
If a checkpoint uncovers a concrete failure requiring cause analysis or repair,
keep the overall checkpoint in Reflect and route only that bounded symptom to
`$shea-symphony-doctor`.

Reflection is a skill behavior, not a CLI subcommand.

## Backlog contract

Backlog is a parking lot and memory surface, not an execution queue. A Backlog item may be rough, overlapping, speculative, stale, or waiting on an experiment. Always explain what it preserves and which question promotion must answer. Do not present its title as an already-scoped task.

Promotion is the conversion point: re-check the seed against current code and Project state, narrow it into a full Issue Forge contract, and move it to Todo only after explicit operator confirmation.

## Bind and choose a mode

Resolve the active repository, workflow, tracker project, default assignee, and supported workflow actions. Do not assume a checkout path, workflow filename, repository, or account.

- Use Reflect when the operator asks for current status, what can proceed next,
  remaining blockers, candidates, or Backlog ideas to extract or organize.
- Use Promote when the operator selects an existing Backlog item.
- Use Doctor only for a concrete malfunction, failed invariant, or bounded
  recovery question; do not convert a routine checkpoint into Doctor triage.
- Ask one short question if the requested mode is ambiguous.

Use configured workflow surfaces for Project fields, state, relationships, and claim locks. Ordinary issue and PR reads may use the configured provider.

## Reflect mode

Read only relevant recent context: current Project state, issues, PRs, Doctor findings, run evidence, and the operator's stated context. Keep repeated dogfood pain, missing workflow invariants, audit gaps, documentation boundaries, or persistent operator friction. Drop duplicates and one-off complaints.

Use a compact, non-dispatchable seed:

```
## Issue Setup

- UAT Required: TBD
- Assignee: <resolved default>
- Dependencies: TBD
- Related Parent Issue or Context: Reflective backlog seed.

## Issue Goal

<one concrete sentence>

## Issue Context

<why this surfaced>

## Current Seed Scope

- <bounded thought>

## Open Questions for Issue Forge

- <what promotion must decide>

## Expected Promotion Path

Discuss through Issue Forge, resolve scope, dependencies, verification, and UAT,
then promote to Todo only if still worthwhile.
```

After explicit confirmation, create it through the guarded Forge action as Backlog: <short title>, with explicit write authority. Do not mutate code.

## Promote mode

Read the selected issue through structured issue inspection and confirm it is still Backlog. Stop if it is already Todo, In Progress, or closed.

Discuss as Issue Forge: goal, why now, scope, guardrails, dependencies, parent/subissue shape, current-state freshness, verification, and UAT. Ask one to three focused questions per turn and show a short promotion-readiness note. For batches, the parent owns final Human Review and UAT; ordinary children pass Agent Review to Merging unless a direct Human Review exception is recorded.

Before drafting, compare the seed with current main, relevant open/done issues, and PRs. If later work solved the gap, recommend closing or retaining Backlog. If the shape changed, promote only the residual slice and record the drift. If freshness cannot be established cheaply, ask whether to scan further, retain Backlog, or proceed with an explicit risk assumption.

After explicit confirmation:

1. Keep the existing issue number.
2. Rewrite it to the full Issue Forge execution contract.
3. Rename it to an executable imperative title.
4. Run the configured guarded promotion action with the operator decision, scope/dependency context, and explicit write authority.
5. Let the action make Backlog to Todo the final mutation, then read back.

For a Human Review contract revision, use the guarded Forge Rework flow instead of promotion or a raw state mutation.
