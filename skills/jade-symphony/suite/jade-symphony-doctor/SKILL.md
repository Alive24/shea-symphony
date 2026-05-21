---
name: jade-symphony-doctor
description: Use when diagnosing Jade Symphony doctor findings, Need Human Input items, issue or PR blockers, and install-health gaps, then giving an explicit repair recommendation and executing confirmed safe repairs in the same session when the workflow contract allows it.
metadata:
  short-description: Jade Symphony doctor triage
  suite-version: 2026.05.18
---

# Jade Symphony Doctor

Use this skill for read-first operator triage around `doctor`, `debug`,
install-health, local recovery findings, and stuck `Need Human Input` issues.
After diagnosis, give one explicit repair recommendation and say whether it can
be executed in the current Codex session.

## Repository

Default repository:

```bash
cd /Volumes/Bohemialive/GitHub/jade-symphony
```

Canonical workflow:

```bash
workflows/jade-symphony.md
```

## Operating Rule

Start with read-only diagnosis:

```bash
cargo run -- project state workflows/jade-symphony.md
cargo run -- doctor workflows/jade-symphony.md
cargo run -- debug workflows/jade-symphony.md
```

For install-health checks, preview or validate the repo-owned suite:

```bash
node scripts/install-jade-symphony-skills.js --dry-run
node scripts/install-jade-symphony-skills.js --validate
```

Report:

- the exact doctor/debug finding;
- whether it is a blocker or warning;
- the safest CLI-owned or installer-owned repair path;
- the exact target issue, PR, worktree, or local skill path;
- whether the repair can be executed in this same session;
- any operator decision still needed before writing.

When an operator has already asked for a specific repair, such as updating the
local Doctor skill, treat that request as confirmation for that bounded write
after printing the target paths. Do not broaden the repair to unrelated skills
unless the operator asked for the whole suite.

For worktree or session ambiguity, use the current grouped command:

```bash
cargo run -- workspace show workflows/jade-symphony.md '#258'
cargo run -- session list workflows/jade-symphony.md
git worktree list --porcelain
```

## Explicit Repair Shape

Do not stop at "route to #242", "use manual merge", or "needs operator". End
with one concrete next action:

- a lane handoff command, such as `$jade-symphony-manual-main`,
  `$jade-symphony-manual-review`, or `$jade-symphony-manual-merge`;
- a Jade Symphony CLI repair command, such as `project set-state`,
  `project link-pr`, `doctor ... repair`, or `project timeline-comment`;
- a local install-health command, such as suite dry-run, validate, or a targeted
  copy/install path;
- one operator question when the evidence still depends on a human decision.

If the repair is confirmed and fits the workflow contract, continue in the same
Codex session. Switch to the owning skill or lane workflow before doing normal
Main, Review, Human Review, or Merging work.

## Boundaries

- Do not start Main, Review, or Merge lane work from this skill.
- Do not mutate Project state unless the operator explicitly approves a
  documented Jade Symphony CLI repair command.
- Doctor triage or repair evidence belongs in a standalone append-only
  `Jade Symphony Doctor Triage` timeline comment. Use
  `project timeline-comment` for operator-authored notes; do not use
  `project workpad`, which is reserved for the persistent Main Agent Workpad.
- Do not silently overwrite local skills; use the suite installer, show target
  paths, and require confirmation before writing.
- Local skill writes are allowed only when the operator explicitly asked for
  them or confirmed the printed target paths. Prefer targeted Doctor-skill
  updates when the request is only about Doctor; use the full suite installer
  only when the operator asks for the whole suite.
