---
name: jade-symphony-manual-merge
description: Use when manually running a Merging Agent session for Jade Symphony merge-lane work from a fresh session. Claims Rework caused by failed merging and Merging issues, repairs existing PR branches when safe, records evidence, and lands approved PRs without sending merge-lane repair back through Agent Review.
metadata:
  short-description: Jade Symphony manual Merging Agent
  suite-version: 2026.05.17
---

# Jade Symphony Manual Merging Agent

Use this skill to operate a human-supervised Jade Symphony Merging Agent
session. The Merging Agent owns merge-lane repair and landing. It does not own
fresh feature implementation or ordinary Todo dispatch.

## Repository

Default repository:

```bash
cd /Volumes/Bohemialive/GitHub/jade-symphony
```

Canonical workflow:

```bash
workflows/jade-symphony.md
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
`$jade-symphony-manual-main` for that.

## Preflight

```bash
cargo run -- project state workflows/jade-symphony.md
cargo run -- doctor workflows/jade-symphony.md
cargo run -- project inspect workflows/jade-symphony.md '#<issue>'
cargo run -- project issue workflows/jade-symphony.md '#<issue>' --json
gh issue view <issue> --repo Alive24/jade-symphony --comments
gh pr view <pr> --repo Alive24/jade-symphony --json number,title,state,url,headRefName,baseRefName,mergeStateStatus,reviewDecision,statusCheckRollup,isDraft,commits,closingIssuesReferences
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
   are already in `Rework`.

Pick the issue only when all are true:

- Status is `Rework` or `Merging`.
- `Merging Agent` field is empty or belongs to this session.
- A linked PR can be identified from Project data, issue comments, PR closing
  references, Main Workpad evidence, or lane timeline comments.
- Prior review and human approval evidence exists, or the issue is routed to
  `Need Human Input` instead of merged.

## Merge-Lane Recovery

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

## Merging

For `Merging` issues:

1. Confirm the PR is open, non-draft, linked to the issue, and targets the
   expected base.
2. Confirm review approval and human approval evidence.
3. Confirm mergeability and checks are clean, or wait/retry if mergeability is
   transiently `UNKNOWN`.
4. Merge using the repository's accepted merge method.
5. Record merge evidence and reconcile issue/Project state to `Done`.

Do not delete the local PR branch during merge. Jade Symphony issue worktrees
intentionally keep that branch checked out for audit and recovery, so branch and
worktree cleanup belongs to explicit Jade Symphony `clean` / workspace cleanup
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

## Status Transition Ordering

Project `Status` changes must be the final mutating step of the session. Before
moving an issue to `Done`, `Need Human Input`, or another routing state, finish
merge evidence, PR/issue reconciliation, append-only `Jade Symphony Merge Run`
timeline comments. Do not delete the local PR branch during merge: issue
worktrees intentionally keep that branch checked out for audit and recovery.
Use Jade Symphony `clean` / workspace cleanup surfaces later for explicit
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
