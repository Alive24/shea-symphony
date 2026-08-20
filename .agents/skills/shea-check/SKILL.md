---
name: shea-check
description: Refresh and assess current Shea Symphony execution posture without changing it. Use when an operator asks whether a named issue can proceed, what is running or blocked, what changed while they were away, what can run in parallel, or which Shea lane or repair Skill should handle the next action.
---

# Shea Check

Provide bounded, read-only operator navigation. Refresh live facts; never use
"refresh" to mean rewriting an Issue contract or tracker state.

## Bind and inspect

Resolve `.shea/contracts/workflow-capability.v1.md`, its active workflow, and a
supported adapter. Bind the current repository, Project, and operator-named
scope.

For a named Issue, prefer targeted reads of its state, native relationships,
lane gate, claims or jobs, linked PR, canonical workspace, and latest durable
evidence. Inspect current code or the Issue contract only when freshness is
material to the decision. Do not reopen broad history.

For a repository checkpoint, report only the active work, concrete blockers,
and a small set of next eligible candidates. Prefer existing App, local, and
targeted tracker surfaces over repeated full-Project loads. Do not turn this
into a Backlog audit.

Separate observed facts from inference. Treat stale claims, missing evidence,
and uncertain external reads as uncertainty, not permission to repair them.

## Decide and route

Give one clear posture for each selected item:

- `ready`: name the eligible lane and recommend its App action or lane Skill;
- `in_flight`: identify the current lane, claim or job, workspace, and next
  expected boundary;
- `blocked`: name the exact unresolved blocker and what would clear it;
- `needs_shaping`: route contract creation, revision, or promotion to
  `$shea-issue-forge`;
- `needs_repair`: route an observed runtime, tracker, workspace, or contract
  failure to `$shea-doctor`;
- `backlog`: route future-work capture or seed maintenance to `$shea-backlog`;
  or
- `lane_handoff`: route `Todo`, `Agent Review`, `Human Review`, or `Merging` to
  the corresponding installed Shea lane Skill.

When several items can proceed, distinguish genuinely independent work from
work that merely uses separate worktrees. Recommend only the smallest useful
parallel set.

## Stop boundary

Do not create or modify Issues, Project fields, relationships, claims, jobs,
workpads, worktrees, branches, or PRs. Do not start a lane. If a contract may be
stale, report the conflicting live fact and route revision to Issue Forge.

End with the conclusion, its evidence anchors, and one concrete next action.
