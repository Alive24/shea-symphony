---
name: shea-symphony-issue-forge-dream
description: Use when slowly mining broader Shea Symphony history, recent runs, workpads, skills, docs, Project state, and memory summaries for evidence-backed Backlog seeds and bounded Dream Logs.
metadata:
  short-description: Deep Shea Symphony backlog mining
  suite-version: 2026.05.22
---

# Shea Symphony Issue Forge Dream

Run slow, deep backlog mining for Shea Symphony. Dream is a separate skill from
Issue Forge Reflect: Reflect is conscious, short-term, and targeted; Dream is
broader, slower, and evidence-heavy.

Dream is a skill behavior, not a Shea Symphony CLI subcommand. Do not expect or
ask for `shea-symphony forge dream`.

## Runtime Topology

Live Shea Symphony dream/reflection shaping still runs through the protected
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
SHEA_SYMPHONY_APP_PROFILE_PATH=/Users/chuntengxiao/Documents/GitHub/shea-symphony/.shea/app-profile.local.json npm run tauri -- dev
```

The profile points at the Shea Symphony target checkout and the MVP CLI binary.
Do not assume `npm run tauri -- dev -- --workdir <path>` alone keeps the backend
on MVP code.

Default assignee:

```text
Alive24
```

## Operating Rules

- Create Backlog seeds by default unless the operator explicitly asks for
  report-only mode.
- Never create Todo issues directly.
- Never promote a Dream candidate to Todo without a later explicit Issue Forge
  promotion discussion.
- Do not mutate Shea Symphony business or CLI code while dreaming.
- Dream Logs are advisory context, not execution authority.
- Dream content becomes executable only after it is promoted into an issue body,
  docs, skill instructions, or a CLI invariant.
- Do not bypass the Shea Symphony CLI for Project state or issue creation.
- Raw GitHub issue/PR reads are acceptable for ordinary content; Project state,
  Project fields, relationships, claim locks, and workflow status should go
  through Shea Symphony CLI when available.
- Avoid backlog noise. Every seed needs a concrete evidence anchor.
- Low-confidence candidates stay in Watchlist or become very light Backlog
  seeds; do not promote them without explicit operator discussion.
- Summarize conversations and sessions. Do not paste raw long conversation
  dumps into Dream Logs.
- Dream may directly improve internal repository documentation when the change
  clarifies Dream findings, operator memory, workflow lessons, run logs, or
  internal maintenance context. Internal documentation includes `docs/dream-log/`
  and other docs whose primary audience is the Shea Symphony operator or
  maintainers.
- Do not directly change repo-owned or locally installed skills while dreaming.
  Skill changes should be captured as proposals, Backlog seeds, or future Issue
  Forge promotion work unless the operator explicitly switches out of Dream and
  asks for a direct skill edit.
- Do not directly change external-facing product surfaces while dreaming.
  External-facing surfaces include root `README.md`, CLI help text, user-facing
  command output, UI, public docs, and workflow behavior. For these, summarize a
  proposal, create or update a Backlog seed, or recommend a future Issue Forge
  promotion instead of editing them in the Dream run.
- After each write-mode Dream round, commit the Dream artifacts and any allowed
  internal documentation updates before starting another Dream round. Do not let
  multiple Dream rounds accumulate uncommitted state in the canonical checkout.
  If the commit cannot be made safely, stop and report the dirty state instead
  of continuing to dream.

## Mode Selection

- Use Dream write mode by default when the operator asks to dream, mine backlog,
  sleep on recent work, or create deep backlog seeds.
- Use report-only mode when the operator explicitly says not to create issues,
  asks for a rehearsal, or asks to preview candidates first.
- Use Issue Forge Reflect instead when the source window is intentionally recent
  and narrow.
- Use Issue Forge Promote when the operator selects an existing Backlog issue
  and wants to turn it into executable Todo work.

## Source Window

Gather a bounded but broad source set. Prefer live reads and repo-owned docs
over memory-derived claims when both are cheap.

Default reads:

```bash
cd /Users/chuntengxiao/.shea-symphony/mvp/shea-symphony-2606-mvp
cargo run -- project-state workflows/shea-symphony.md
cargo run -- inspect workflows/shea-symphony.md
cargo run -- doctor workflows/shea-symphony.md
```

Then sample relevant sources:

- current `Todo`, `Rework`, `Backlog`, and `Agent Review` items;
- recent `Done` issues and linked PR, Main Workpad, and timeline evidence;
- run logs, session registry evidence, Doctor findings, and debug reports;
- `docs/dream-log/INDEX.md` plus the most recent five Dream run directories;
- repo-owned skills under `skills/shea-symphony/suite/`;
- local installed skills only when install drift is relevant;
- recent rollout summaries or conversation memory summaries when available;
- original design docs under `docs/bootstrap/`;
- recent docs/skill/code changes that indicate drift or repeated friction.

Do not reread unlimited historical logs by default. The default source window
includes `docs/dream-log/INDEX.md` plus the most recent five Dream run
directories, not merely five files. Use that as the normal compaction boundary.
Reopen older runs only when the index points to them or the current theme
depends on them.

## Candidate Triage

Keep candidates when they show one or more of:

- repeated dogfood pain;
- missing lane or tracker invariants;
- stale skill/docs contracts;
- unsafe manual recovery patterns;
- duplicate or noisy Backlog pressure;
- missing evidence surfaces;
- operator friction that recurs across issues or reviews.

Drop or Watchlist candidates when they are:

- one-off annoyances with no evidence anchor;
- already covered by an open issue or recently landed PR;
- too broad to seed safely;
- execution work disguised as reflection;
- low-confidence speculation from memory only.

For each kept candidate, record:

- evidence anchors;
- existing coverage checked;
- likely lane or owner;
- promotion path;
- Dream confidence: `High`, `Medium`, `Low`, or `Watchlist`.

## Backlog Seed Shape

Use enriched Backlog seeds. Keep them non-dispatchable but useful enough for a
future Issue Forge promotion.

```md
## Issue Setup

- UAT Required: TBD
- Assignee: Alive24
- Dependencies: TBD
- Related Parent Issue or Context: Dream seed from `docs/dream-log/YYYY-MM-DD-<run-count>-<slug>/RUN.md`.

## Issue Goal

[One concrete sentence.]

## Dream Evidence Anchors

- `path/or/issue`: [short evidence summary]

## Existing Coverage Checked

- [What issue, PR, doc, skill, or command already covers part of this.]

## Current Seed Scope

- ...

## Non-Goals / Guardrails

- ...

## Promotion Path

Discuss through Issue Forge, resolve scope / dependencies / verification / UAT,
then promote to Todo only if still worth doing.

## Dream Confidence

Medium - [short reason]
```

Create seeds through the CLI:

```bash
cargo run -- forge create \
  --workflow workflows/shea-symphony.md \
  --title "Backlog: <short title>" \
  --body-file /private/tmp/<slug>.md \
  --status Backlog \
  --assignee Alive24 \
  --write
```

Aim for 3-8 Backlog seeds when a Dream run has enough strong candidates. Fewer
is better than noisy filler. If there are no strong candidates, create none and
explain why.

## Dream Log Layout

Write Dream Logs under:

```text
docs/dream-log/YYYY-MM-DD-<run-count>-<slug>/
```

Each run directory supports:

- `RUN.md`: source inventory, round summary, created backlog mapping, sleep
  enough judgment, Gemini review status, and next Dream theme.
- `topic-*.md`: bounded topic logs with evidence, candidate triage, coverage
  checks, and confidence. Keep each topic log to a soft 250-line limit.
- `gemini-review.md`: lightweight Gemini review summary or explicit unavailable
  reason.
- `created-backlog.md`: optional mapping when many seeds are created.

Update `docs/dream-log/INDEX.md` after every write-mode Dream run. Keep the
index compact: run path, date, themes, created issue numbers, watchlist themes,
and archive notes. The index should point future Dream runs to older context
instead of forcing linear rereads.

Standard issue references:

- `Dream Log: docs/dream-log/YYYY-MM-DD-<run-count>-<slug>/RUN.md`
- `Dream Topic: docs/dream-log/YYYY-MM-DD-<run-count>-<slug>/topic-*.md`

## Dream Rounds

Dream may run multiple bounded rounds. Each round must report:

- whether this round slept enough;
- Dream Logs written;
- Backlog seeds created;
- Watchlist candidates retained;
- Gemini review status;
- next most valuable Dream theme if more sleep is useful.

Before starting a subsequent write-mode round:

1. Review `git status --short --branch`.
2. Commit the completed Dream round's `docs/dream-log/` artifacts and any
   allowed internal documentation updates.
3. Confirm the canonical checkout is clean after the commit.
4. Only then begin the next Dream round.

Stop when another round would mostly reread the same evidence or create noisy
candidates. Say `slept enough: yes` only when the current source window produced
no high-value unexplored theme.

## Gemini Review

Run a lightweight Gemini review by default because Dream is not time-sensitive.
Use the configured local Gemini command when available. If Gemini cannot run,
write `gemini-review.md` with the command tried, the failure reason, and what a
later reviewer should check.

The Gemini review should check:

- duplicate risk;
- evidence quality;
- whether candidate scope is too broad;
- whether created Backlog seeds are safely non-dispatchable;
- whether any Dream Log content risks becoming accidental lane authority.

Do not let Gemini create issues or mutate tracker state directly.

## Lane Reading Rules

- Dream, Reflect, and Issue Forge may actively read Dream Logs.
- Main reads only Dream Logs explicitly referenced by the issue contract.
- Review reads relevant Dream Logs only when the issue body or PR changes
  involve Dream-derived context.
- Merge generally does not read Dream Logs unless PR changes Dream docs or the
  issue contract requires it.
- Doctor may use Dream Logs as advisory context only, never as workflow
  invariants.

If a lane needs an invariant, promote the Dream learning into docs, skill
instructions, issue body acceptance criteria, or CLI behavior first.

## Report-Only Rehearsal

When report-only mode is requested:

- read the same source window;
- draft the source inventory and topic triage;
- show would-create Backlog seeds with confidence and duplicate checks;
- do not write Dream Logs unless the operator asks for a rehearsal artifact;
- do not run `forge create`.

End with a concise recommendation: write-mode now, sleep another round, or keep
the candidates as Watchlist.
