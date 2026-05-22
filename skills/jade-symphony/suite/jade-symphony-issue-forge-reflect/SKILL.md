---
name: jade-symphony-issue-forge-reflect
description: Use when reflecting over recent Jade Symphony conversations, Project state, dogfood logs, or work records to extract issue backlog candidates, create them as non-dispatchable Project Backlog drafts, or promote existing Backlog drafts through conversational Issue Forge into executable Todo issues.
metadata:
  short-description: Reflect Jade Symphony backlog into forgeable issues
  suite-version: 2026.05.22
---

# Jade Symphony Issue Forge Reflect

Turn loose recent context into a manageable Jade Symphony Backlog, then help
promote selected Backlog drafts into executable issues.

Reflection is a skill behavior, not a Jade Symphony CLI subcommand. Do not
expect or ask for `jade-symphony forge reflect`.

## Repository

Default repo:

```bash
/Volumes/Bohemialive/GitHub/jade-symphony
```

Canonical workflow:

```bash
workflows/jade-symphony.md
```

Default assignee:

```text
Alive24
```

## Operating Rules

- Do not treat Backlog items as executable work.
- Do not move a Backlog item to `Todo` without explicit operator confirmation
  after discussion.
- Do not bypass the Jade Symphony CLI with raw Project mutations.
- Raw GitHub issue/PR reads are acceptable for context; Project state, Project
  fields, relationships, claim locks, and workflow status must go through the
  Jade Symphony CLI when available.
- Prefer small seed issues over over-designed contracts during reflection.
- Use `$jade-symphony-issue-forge` issue-body standards when promotion starts.
- In Promote mode, default to editing the existing Backlog issue in place.
- Do not mutate code while using this skill unless the user explicitly changes
  the task.

## Mode Selection

- Use Reflect mode when the user asks to extract, organize, or seed Backlog ideas.
- Use Promote mode when the user points at a Backlog item and wants to refine or
  make it executable.
- If unclear, ask one short question about reflect versus promote.

## Reflect Mode

Gather only relevant sources:

```bash
cd /Volumes/Bohemialive/GitHub/jade-symphony
cargo run -- project state workflows/jade-symphony.md
cargo run -- project issue workflows/jade-symphony.md '#<number>' --json
cargo run -- doctor workflows/jade-symphony.md
```

Keep candidates if they are repeated dogfood pain, missing workflow rules, CLI
surfaces, operator skills, audit invariants, or documentation boundaries. Drop
duplicates and one-off complaints.

Use this compact seed body:

```md
## Issue Setup

- UAT Required: TBD
- Assignee: Alive24
- Dependencies: TBD
- Related Parent Issue or Context: Reflective backlog seed from recent Jade Symphony work.

## Issue Goal

[One concrete sentence.]

## Issue Context

[Why this surfaced.]

## Current Seed Scope

- ...

## Open Questions for Issue Forge

- ...

## Expected Promotion Path

Discuss with the operator through Issue Forge, resolve scope / dependencies /
verification, then promote to an executable Todo issue if still worth doing.
```

After explicit confirmation, create the seed:

```bash
cargo run -- forge create \
  --workflow workflows/jade-symphony.md \
  --title "Backlog: <short title>" \
  --body-file /private/tmp/<slug>.md \
  --status Backlog \
  --assignee Alive24 \
  --write
```

## Promote Mode

Read the Backlog item first:

```bash
cd /Volumes/Bohemialive/GitHub/jade-symphony
cargo run -- project issue workflows/jade-symphony.md '#<number>' --json
```

Confirm it is still `Backlog`. If it is already `Todo`, `In Progress`, or
closed, stop and explain.

Discuss like Issue Forge:

- Ask 1-3 focused questions per turn.
- Resolve goal, why now, scope, guardrails, dependencies, parent/subissue shape,
  current code-state freshness, verification, and UAT.
- If promotion uses a native parent/subissue batch, make the parent issue the
  final Human Review and UAT owner. Routine native subissues should keep
  independent Agent Review, default to no direct UAT/Human Review, and route
  passing review to `Merging`. Record `Subissue Human Review Exception:
  <reason>` only for child slices that truly need direct Human Review.
- After each question round, include a short promotion-readiness note.
- Do not promote until the operator explicitly confirms promotion.

Before drafting a promoted Todo contract, compare the Backlog seed against the
current development state, not only against the seed text:

- Check enough latest repo/project context to decide whether the original gap
  still exists on current `main`.
- Search existing open/done issues or PRs when later work may already cover the
  gap.
- If the gap is already solved, recommend closing or leaving the item in
  `Backlog` instead of creating make-work.
- If later code changed the shape of the gap, promote only the residual slice
  and record the drift in the promoted issue context.
- If freshness cannot be determined cheaply, ask whether to scan more, keep the
  item in `Backlog`, or promote with an explicit freshness-risk assumption.

Default promotion path:

1. Keep the same issue number.
2. Rewrite the body into the full Issue Forge execution contract.
3. Rename the title to an executable imperative title.
4. Move Project `Status` from `Backlog` to `Todo` through Jade Symphony CLI.
5. Let `forge promote` write the structured Promotion Note.

The `Backlog` to `Todo` status change must be the final mutating step of the
promotion session. After `forge promote --write`, only read back and report.

If reflection identifies a live `Human Review` issue whose contract must be
revised, treat that as Issue Forge discussion, not Backlog promotion. Prepare
the full replacement body and evidence file, require explicit operator
confirmation, and use `forge rework`; do not use `forge promote`, `set-state`,
or raw Project mutation for the normal path.

Suggested command after confirmation:

```bash
cargo run -- forge promote <number> \
  --workflow workflows/jade-symphony.md \
  --title "<executable title>" \
  --body-file /private/tmp/<promoted-body>.md \
  --operator-confirmation "<exact confirmation>" \
  --decision "<key operator decision>" \
  --scope-change "<major change from seed>" \
  --dependency-context "<dependencies and related context>" \
  --write
```
