---
name: jade-symphony-doctor
description: Use when triaging Jade Symphony doctor output, local install-health gaps, or Need Human Input recovery paths without starting implementation, review, or merge lane work.
metadata:
  short-description: Jade Symphony doctor triage
  suite-version: 2026.05.17
---

# Jade Symphony Doctor

Use this skill for read-first operator triage around `doctor`, `debug`,
install-health, and local recovery findings.

This skill is a read-first operator triage entrypoint. The Jade Symphony CLI
`doctor` command reports local install-health warnings, while the skill keeps
repair decisions explicit and operator-confirmed.

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
- the safest CLI-owned repair path;
- any operator decision needed before writing.

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
- Keep automatic install-health repair out of this skill; `doctor` should
  diagnose and point to the #242 install/update path rather than rewriting
  local skill files.
