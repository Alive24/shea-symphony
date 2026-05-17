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

This is a stub slot in the `2026.05.17` skill suite. Full automatic
install-health checks and prompts remain future work for issue #256. Do not
smuggle #256 implementation into unrelated lane work.

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
cargo run -- project-state workflows/jade-symphony.md
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
- Do not silently overwrite local skills; use the suite installer, show target
  paths, and require confirmation before writing.
- Keep automatic install-health repair out of this issue slice; track it under
  #256.
