---
name: shea-symphony-doctor
description: Use when diagnosing Shea Symphony doctor findings, Need Human Input items, issue or PR blockers, install-health gaps, and interrupted workflow state, then leading the smallest confirmed recovery in the same session when the workflow contract allows it.
metadata:
  short-description: Shea Symphony doctor triage
  suite-version: 2026.05.23
---

# Shea Symphony Doctor

Use this skill for read-first operator triage around `doctor`, `debug`,
install-health, local recovery findings, and stuck `Need Human Input` issues.
After diagnosis, give one explicit repair recommendation and say whether it can
be executed in the current Codex session. Doctor is the operator-facing owner
of abnormal recovery: do not require a second recovery skill merely because the
repair touches work normally owned by a lane.

## Repository

Default repository:

```bash
cd /Volumes/Bohemialive/GitHub/shea-symphony
```

Canonical workflow:

```bash
.shea/workflows/shea-symphony.md
```

## Operating Rule

Start with read-only diagnosis:

```bash
cargo run -- project state .shea/workflows/shea-symphony.md
cargo run -- autopilot plan .shea/workflows/shea-symphony.md
cargo run -- doctor .shea/workflows/shea-symphony.md
cargo run -- debug .shea/workflows/shea-symphony.md
```

For install-health checks, preview or validate the repo-owned suite:

```bash
node scripts/install-shea-symphony-skills.js --dry-run
node scripts/install-shea-symphony-skills.js --validate
```

Report:

- the exact doctor/debug finding;
- whether it is a blocker or warning;
- the safest CLI-owned or installer-owned repair path;
- the exact target issue, PR, worktree, or local skill path;
- whether the repair can be executed in this same session;
- whether the work is bounded recovery or new lane execution;
- any operator decision still needed before writing.

When an operator has already asked for a specific repair, such as updating the
local Doctor skill, treat that request as confirmation for that bounded write
after printing the target paths. Do not broaden the repair to unrelated skills
unless the operator asked for the whole suite.

For worktree or session ambiguity, use the current grouped command:

```bash
cargo run -- workspace show .shea/workflows/shea-symphony.md '#258'
cargo run -- session list .shea/workflows/shea-symphony.md
git worktree list --porcelain
```

## Explicit Repair Shape

Do not stop at "route to #242", "use manual merge", or "needs operator". End
with one concrete next action:

- a bounded same-session recovery plan led by this Doctor skill;
- the normal all-lane foreground command, when no focused repair is needed:
  `cargo run -- autopilot loop .shea/workflows/shea-symphony.md --max-iterations 1 --write`;
- a Shea Symphony CLI repair command, such as `project set-state`,
  `project link-pr`, `doctor ... repair`, or `project timeline-comment`;
- a local install-health command, such as suite dry-run, validate, or a targeted
  copy/install path;
- one operator question when the evidence still depends on a human decision.

If the repair is confirmed and fits the workflow contract, continue in the same
Codex session. Use Shea Symphony CLI commands as deterministic inspection and
mutation primitives; the skill owns diagnosis, sequencing, safety decisions,
and final readback. Do not hand the recovery to another skill simply to finish
the repair.

## Bounded Same-Session Recovery

A recovery is bounded when all of the following are true:

- it keeps the selected issue and its accepted scope unchanged;
- it reuses or safely reconciles the issue's existing session, worktree,
  branch, PR, and tracker evidence;
- it performs only the smallest code, configuration, publication, or tracker
  changes needed to restore a valid workflow boundary; and
- it stops at the next normal lane boundary without performing independent
  review, human approval, or merge work.

After the operator confirms the printed repair targets, Doctor may inspect and
repair the existing work, run proportionate verification, create or repair the
expected PR, link it through the configured Project surface, append standalone
Doctor triage evidence, reconcile stale runtime state, and apply the documented
Project transition. Read back every external mutation before reporting success.

For an interrupted Main handoff, prefer this sequence:

1. Inspect the issue, runtime/session ownership, worktree, branch, commits, and
   linked PR without writing.
2. Decide whether the existing work satisfies the unchanged issue scope. If it
   does not, describe the smallest remaining repair and obtain confirmation
   before editing.
3. Repair and verify the existing branch in its owned worktree.
4. Create or repair the ready PR and link it using the configured Project
   command surface.
5. Append a `Shea Symphony Doctor Triage` note containing the observed failure,
   repair actions, verification, and intended terminal state.
6. Reconcile stale runtime/session evidence and move the issue to the next
   contract-valid state only after all handoff invariants pass.
7. Read back the issue, PR, Project state, and runtime state.

This does not turn Doctor into a normal Main, Review, or Merge entrypoint. If
the request changes product scope, starts unrelated implementation, requires
independent review judgment, requires human UAT, or authorizes a merge, stop and
use the normal workflow lane rather than stretching the recovery boundary.

## Boundaries

- Do not start normal queued Main work, conduct independent Review or Human
  Review, or merge from this skill. Bounded recovery of already selected work is
  explicitly allowed as described above.
- Do not mutate Project state unless the operator explicitly approves a
  documented Shea Symphony CLI repair command.
- Doctor triage or repair evidence belongs in a standalone append-only
  `Shea Symphony Doctor Triage` timeline comment. Use
  `project timeline-comment` for operator-authored notes; do not use
  `project workpad`, which is reserved for the persistent Main Agent Workpad.
- Do not silently overwrite local skills; use the suite installer, show target
  paths, and require confirmation before writing.
- Local skill writes are allowed only when the operator explicitly asked for
  them or confirmed the printed target paths. Prefer targeted Doctor-skill
  updates when the request is only about Doctor; use the full suite installer
  only when the operator asks for the whole suite.
