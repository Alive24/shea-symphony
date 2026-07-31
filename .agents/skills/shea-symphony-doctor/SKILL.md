---
name: shea-symphony-doctor
description: Diagnose Shea Symphony workflow, install-health, and blocked-issue findings; recommend one concrete repair and perform only the explicitly confirmed, bounded repair.
---

# Shea Symphony Doctor

Use this skill for read-first operator triage of Doctor findings, debug output, install-health drift, stuck Need Human Input items, and issue or PR blockers. It diagnoses and hands off; it does not silently become a Main, Review, or Merge lane.

## Bind the runtime first

Resolve the active repository, workflow definition, tracker project, command surface, and relevant issue, PR, session, worktree, or skill path. Never assume a checkout path, workflow filename, installer, or executable. If a necessary binding cannot be discovered, make that absence the finding and ask for the smallest missing fact.

Use read-only workflow, project, plan, Doctor, debug, and Git worktree inspection first. Treat configured workflow actions as authoritative for workflow state; ordinary issue and PR reads may use the configured provider.

## Diagnose and recommend

For each finding, state:

- the exact observed evidence and whether it is a blocker or warning;
- the affected issue, PR, worktree, session, or installed-skill path;
- the likely cause and confidence level;
- the safest workflow-owned repair path;
- whether it is safe to perform in this session; and
- the one operator decision still needed, if any.

End with one concrete next step, not a vague route:

- a named lane handoff, such as $shea-symphony-manual-main,
  $shea-symphony-manual-review, or $shea-symphony-manual-merge;
- the configured foreground workflow action;
- a documented state, PR-link, worktree, or install-health repair; or
- one focused operator question.

When the operator already requested a specific bounded repair, that request is confirmation only for that repair. Print the target paths or tracker mutation before writing and do not expand the scope to unrelated skills or issues.

## Evidence and boundaries

Write diagnosis and repair evidence as a standalone, append-only Shea Symphony Doctor Triage timeline note through the configured workflow surface. Do not overwrite the Main Agent Workpad.

- Do not start normal Main, Review, Human Review, or Merge work from Doctor.
- Do not change Project state unless the operator explicitly confirms the documented repair.
- Do not overwrite local or repo-scoped skills. Use a targeted, confirmed update; use a whole-suite installer only when the operator asks for it.
- For ambiguous worktrees or sessions, inspect grouped workspace/session evidence and local Git metadata before suggesting any repair.
- After a confirmed repair, read back the result and report any remaining risk.
