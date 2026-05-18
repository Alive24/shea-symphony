# Jade Symphony Dream Log Index

This index is the compact entrypoint for repo-owned Dream Logs. Dream reads this
file plus the most recent five Dream run directories by default; older runs are
opened only when the index points to them or the current theme depends on them.

Dream Logs are advisory context. They do not authorize Main, Review, Merge, or
Doctor lane actions until a learning is promoted into an issue body, docs, skill
instructions, or a CLI invariant.

## Current Runs

- `2026-05-18-01-issue-295-dream-skill-rehearsal/`
  - Themes: manual lane claim parsing, session registry status drift, malformed
    claim repair UX.
  - Created Backlog: #297, #298, #299.
  - Slept enough: no; this was an issue-scoped rehearsal, not a full broad
    Dream over recent Jade Symphony history.
  - Gemini review: ran; duplicate/scope/lane-authority passed, evidence detail
    follow-up applied in the run/topic logs.

## Source Window Rule

- Always read this index first.
- Then read the most recent five directories matching
  `docs/dream-log/YYYY-MM-DD-<run-count>-<slug>/`.
- Prefer each run's `RUN.md` before opening topic logs.
- Open older runs only when linked from this index, referenced by a current
  issue contract, or needed to resolve duplicate/coverage checks.

## Run Directory Shape

Each Dream run directory may contain:

- `RUN.md`: source inventory, created backlog mapping, sleep-enough judgment,
  Gemini review status, and next theme.
- `topic-*.md`: bounded topic logs with evidence anchors, candidate triage,
  existing coverage checks, promotion path, and Dream confidence.
- `gemini-review.md`: lightweight Gemini review summary or explicit unavailable
  reason.
- `created-backlog.md`: optional mapping when several seeds are created.

Topic logs have a soft 250-line human-readability limit. Prefer summaries and
evidence pointers over raw long conversation dumps.

## Issue Reference Format

Use these exact references in Dream-created Backlog seeds:

- `Dream Log: docs/dream-log/YYYY-MM-DD-<run-count>-<slug>/RUN.md`
- `Dream Topic: docs/dream-log/YYYY-MM-DD-<run-count>-<slug>/topic-*.md`

## Lane Reading Boundaries

- Dream, Reflect, and Issue Forge may actively read Dream Logs.
- Main reads only Dream Logs explicitly referenced by the issue contract.
- Review reads relevant Dream Logs only when the issue body or PR changes
  involve Dream-derived context.
- Merge generally does not read Dream Logs unless PR changes Dream docs or the
  issue contract requires it.
- Doctor may use Dream Logs as advisory context only, never as workflow
  invariants.

## Archive Notes

When the index grows, compact older groups into short theme summaries here and
keep links to the run directories. Do not delete run directories as part of
normal Dream compaction; cleanup is a separate operator decision.
