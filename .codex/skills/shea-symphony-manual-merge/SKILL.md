---
name: shea-symphony-manual-merge
description: Use when manually running a Merging Agent session for Shea Symphony merge-lane work from a fresh session. Claims Merging issues or operator-selected historical merge-lane recovery issues, repairs existing PR branches when safe, records evidence, and lands approved PRs without sending merge-lane repair back through Agent Review.
metadata:
  short-description: Shea Symphony manual Merging Agent
  suite-version: 2026.05.22
---

# Shea Symphony Manual Merging Agent

Use this skill to operate a human-supervised Shea Symphony Merging Agent
session. The Merging Agent owns merge-lane repair and landing. It does not own
fresh feature implementation or ordinary Todo dispatch.

## Repository

Default repository:

```bash
cd /Volumes/Bohemialive/GitHub/shea-symphony
```

Canonical workflow:

```bash
workflows/shea-symphony.md
```

Canonical Merge Agent prompt:

```bash
workflows/prompts/merge-agent.md
```

## Operating Rule

Before doing any work:

1. Refresh tracker state from GitHub Project v2 and local runtime state.
2. Respect the `Merging Agent` Project field as the claim lock.
3. Use `Main Agent` as a do-not-touch signal unless the issue is explicitly in
   merge-lane recovery.
4. Preserve existing Human Review and Agent Review evidence.
5. Prefer repairing the existing PR branch over creating replacement work.

Handle only:

- `Merging` issues that are ready to land or need merge-lane diagnosis.
- Historical or explicitly operator-selected merge-lane recovery issues that
  are already in `Rework`. New automated merge-loop routing should prefer
  staying in `Merging` for safe stale-branch retry or moving to
  `Need Human Input` for ambiguous conflict/check failures.

Do not use this skill for fresh `Todo` implementation. Use
`$shea-symphony-manual-main` for that.

## Preflight

```bash
cargo run -- project state workflows/shea-symphony.md
cargo run -- doctor workflows/shea-symphony.md
cargo run -- project inspect workflows/shea-symphony.md '#<issue>'
cargo run -- project issue workflows/shea-symphony.md '#<issue>' --json
gh issue view <issue> --repo Alive24/shea-symphony --comments
gh pr view <pr> --repo Alive24/shea-symphony --json number,title,state,url,headRefName,baseRefName,mergeStateStatus,reviewDecision,statusCheckRollup,isDraft,commits,closingIssuesReferences
```

If the PR is not listed in `closingIssuesReferences`, do not assume it is
missing. Check issue comments, Main Workpad evidence, and lane timeline comments for the
canonical link.

## Selection

Prefer work in this order:

1. `Merging` issues with clean, approved PRs.
2. `Merging` issues needing diagnosis because mergeability, checks, PR linkage,
   or evidence is unclear.
3. Historical or explicitly operator-selected merge-lane recovery issues that
   are already in `Rework`; do not create new Rework routing for merge-loop
   transport repair.

Pick the issue only when all are true:

- Status is `Rework` or `Merging`.
- `Merging Agent` field is empty or belongs to this session.
- A linked PR can be identified from Project data, issue comments, PR closing
  references, Main Workpad evidence, or lane timeline comments.
- Prior review and human approval evidence exists, or the issue is routed to
  `Need Human Input` instead of merged.

## Merge-Lane Recovery

Clean merge is CLI-owned and non-LLM. Do not start Codex, Gemini, tmux, or
app-server just to land a clean approved PR; use `merge loop` / `merge once`
and preserve its direct `gh pr merge` behavior.

For historical or operator-selected merge-lane recovery:

1. Claim through the `Merging Agent` field.
2. Resume the existing PR branch/worktree when possible.
3. Repair conflicts, stale base, or merge-only failures without changing product
   scope.
4. Re-run focused verification.
5. Push the existing PR branch.
6. Record repair evidence.
7. Continue toward landing only if approval remains valid and the change is
   merge-lane-only.

Do not send merge-lane-only repair back to `Agent Review` just because the
branch was rebased or conflicts were resolved.

When a merge-agent runtime session is explicitly needed after a structured
`Merging Agent` claim, the default session backend is Codex app-server through
`merge_lane.agent_backend: codex` and `codex.command: codex app-server -c 'service_tier="fast"'`. Use
`merge_lane.agent_backend: tmux` only as an explicit fallback/debug choice.

For automated interrupted merge-loop recovery, prefer:

```bash
cargo run -- merge loop workflows/shea-symphony.md --max-iterations 1 --max-concurrent 2 --write
```

`merge loop --write` adopts interrupted structured merge-loop/goal claims first
by default, then continues normal merge selection. It must not adopt manual
claims, and it must not route merge repair through `Rework`. Use `--no-recover`
only for debugging or a deliberately conservative operator pass.

Write-mode merge commands may automatically refresh the canonical checkout with
a canonical-only `git merge --ff-only` when clean local `main` is behind its
configured upstream. Treat that as control-surface synchronization only. It does
not refresh PR branches or issue worktrees; those remain merge-lane freshness or
conflict-repair work.

## Merging

For `Merging` issues:

1. Confirm the PR is open, non-draft, linked to the issue, and targets the
   expected base.
2. Confirm review approval and human approval evidence.
3. Confirm mergeability and checks are clean, or wait/retry if mergeability is
   transiently `UNKNOWN`.
4. Merge using the repository's accepted merge method.
5. Record merge evidence and reconcile issue/Project state to `Done`.

Do not delete the local PR branch during merge. Shea Symphony issue worktrees
intentionally keep that branch checked out for audit and recovery, so branch and
worktree cleanup belongs to explicit Shea Symphony `clean` / workspace cleanup
surfaces.

If `mergeStateStatus` is `UNKNOWN`, wait briefly and re-run the same `gh pr view`
query before making a routing decision. Only merge after the status returns
`CLEAN`.

If `mergeStateStatus` is `BEHIND`, prefer the same safe branch-update behavior
as automated `merge once`: update the PR branch without rewriting history,
record evidence, and leave the issue in `Merging` for a later retry. If
`mergeStateStatus` is `DIRTY` or checks are failing, do not default to `Rework`;
attempt repair only when the existing PR worktree is clean and the base can be
merged without rewriting history or leaving uncommitted changes. Otherwise,
record one concrete `Need Human Input` question unless the operator confirms a
different merge-lane-only repair path.

For native subissue PRs, treat dirty or conflicted mergeability as merge-lane
repair work first. Attempt the safe existing-worktree repair before asking for
human input, and keep successful mechanical repair in `Merging` for retry.
Escalate only unresolved conflicts, semantic choices, dirty starting worktrees,
missing worktree evidence, or verification-failing repairs to
`Need Human Input`. Do not route native subissue merge repair to `Rework`.

## Status Transition Ordering

Project `Status` changes must be the final mutating step of the session. Before
moving an issue to `Done`, `Need Human Input`, or another routing state, finish
merge evidence, PR/issue reconciliation, append-only `Shea Symphony Merge Run`
timeline comments. Do not delete the local PR branch during merge: issue
worktrees intentionally keep that branch checked out for audit and recovery.
Use Shea Symphony `clean` / workspace cleanup surfaces later for explicit
cleanup decisions. After status changes, do only readback verification such as
`project issue`, `project state`, or `doctor`.

## Hard Boundaries

- Never claim fresh `Todo` implementation.
- Never use the `Main Agent` field.
- Never create a new feature branch for merge-lane work unless the existing
  branch is unrecoverable and the operator explicitly agrees.
- Never merge without approval evidence.
- Never hide unknown mergeability, missing PR linkage, or missing context.
- Never mark `Human Review` yourself as a substitute for actual review approval.
- Never edit, overwrite, or restructure the Main Agent Workpad; merge evidence
  belongs in standalone timeline comments.
