---
name: shea-symphony-issue-forge-reflect
description: Use when reflecting over recent Shea Symphony conversations, Project state, dogfood logs, or work records to extract issue backlog candidates, create them as non-dispatchable Project Backlog drafts, or promote existing Backlog drafts through conversational Issue Forge into executable Todo issues.
metadata:
  short-description: Reflect Shea Symphony backlog into forgeable issues
  suite-version: 2026.05.22
---

# Shea Symphony Issue Forge Reflect

Turn loose recent context into a manageable Shea Symphony Backlog, then help
promote selected Backlog drafts into executable issues.

Reflection is a skill behavior, not a Shea Symphony CLI subcommand. Do not
expect or ask for `shea-symphony forge reflect`.

## Backlog Semantics

Shea Symphony `Backlog` is a parking lot and memory surface, not an execution
queue. A Backlog item means "there is probably useful work here, but the shape,
priority, dependencies, UAT, or dispatchability still needs operator discussion."

Backlog items may be intentionally rough, stale, overlapping, speculative, or
waiting on another lane experiment. They are not claims, agent work orders,
implementation commitments, priorities, or proof that the work should start.

Promotion is the conversion point: re-check the seed against current code and
Project state, explain what the Backlog item was preserving, narrow it into a
Todo-ready contract, and only then move it to `Todo` after explicit operator
confirmation.

When listing or selecting Backlog candidates, include a short explanation of why
each item was parked in Backlog and what question promotion must answer. Do not
present Backlog titles as if they are already scoped executable tasks.

## Runtime Topology

Live Shea Symphony reflection and issue shaping still runs through the protected
2606 MVP runtime until 2607 replaces the runtime spine. Use the MVP worktree for
CLI/App execution and the active Shea Symphony development worktree only as
source context.

MVP runtime worktree:

```bash
/Users/chuntengxiao/.shea-symphony/mvp/shea-symphony-2606-mvp
```

Canonical workflow inside the MVP runtime:

```bash
workflows/shea-symphony.md
```

If the Tauri App is needed, start it from the MVP runtime `app/` directory with
the local MVP profile:

```bash
cd /Users/chuntengxiao/.shea-symphony/mvp/shea-symphony-2606-mvp/app
SHEA_SYMPHONY_APP_PROFILE_PATH=/Users/chuntengxiao/.shea-symphony/mvp/app-profile-shea-symphony.json npm run tauri -- dev
```

The profile points at the Shea Symphony target checkout and the MVP CLI binary.
Do not assume `npm run tauri -- dev -- --workdir <path>` alone keeps the backend
on MVP code.

Default assignee:

```text
Alive24
```

## Operating Rules

- Do not treat Backlog items as executable work.
- Do not move a Backlog item to `Todo` without explicit operator confirmation
  after discussion.
- Do not bypass the Shea Symphony CLI with raw Project mutations.
- Raw GitHub issue/PR reads are acceptable for context; Project state, Project
  fields, relationships, claim locks, and workflow status must go through the
  Shea Symphony CLI when available.
- Prefer small seed issues over over-designed contracts during reflection.
- Use `$shea-symphony-issue-forge` issue-body standards when promotion starts.
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
cd /Users/chuntengxiao/.shea-symphony/mvp/shea-symphony-2606-mvp
cargo run -- project state workflows/shea-symphony.md
cargo run -- project issue workflows/shea-symphony.md '#<number>' --json
cargo run -- doctor workflows/shea-symphony.md
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
- Related Parent Issue or Context: Reflective backlog seed from recent Shea Symphony work.

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
  --workflow workflows/shea-symphony.md \
  --title "Backlog: <short title>" \
  --body-file /private/tmp/<slug>.md \
  --status Backlog \
  --assignee Alive24 \
  --write
```

## Promote Mode

Read the Backlog item first:

```bash
cd /Users/chuntengxiao/.shea-symphony/mvp/shea-symphony-2606-mvp
cargo run -- project issue workflows/shea-symphony.md '#<number>' --json
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
4. Move Project `Status` from `Backlog` to `Todo` through Shea Symphony CLI.
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
  --workflow workflows/shea-symphony.md \
  --title "<executable title>" \
  --body-file /private/tmp/<promoted-body>.md \
  --operator-confirmation "<exact confirmation>" \
  --decision "<key operator decision>" \
  --scope-change "<major change from seed>" \
  --dependency-context "<dependencies and related context>" \
  --write
```
