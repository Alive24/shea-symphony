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

- `Rework` issues when the rework came from failed merging, stale base, dirty
  mergeability, conflict repair, failing merge checks, or missing merge evidence.
- `Merging` issues that are ready to land or need merge-lane diagnosis.

Do not use this skill for fresh `Todo` implementation. Use
`$jade-symphony-manual-main` for that.

## Preflight

```bash
cargo run -- project-state workflows/jade-symphony.md
cargo run -- doctor workflows/jade-symphony.md
cargo run -- inspect workflows/jade-symphony.md
cargo run -- project-issue workflows/jade-symphony.md '#<issue>' --json
gh issue view <issue> --repo Alive24/jade-symphony --comments
gh pr view <pr> --repo Alive24/jade-symphony --json number,title,state,url,headRefName,baseRefName,mergeStateStatus,reviewDecision,statusCheckRollup,isDraft,commits,closingIssuesReferences
```

If the PR is not listed in `closingIssuesReferences`, do not assume it is
missing. Check issue comments and Jade Symphony workpad evidence for the
canonical link.

## Selection

Prefer work in this order:

1. `Rework` that clearly came from failed merging.
2. `Merging` issues with clean, approved PRs.
3. `Merging` issues needing diagnosis because mergeability, checks, PR linkage,
   or evidence is unclear.

Pick the issue only when all are true:

- Status is `Rework` or `Merging`.
- `Merging Agent` field is empty or belongs to this session.
- A linked PR can be identified from Project data, issue comments, PR closing
  references, or workpad evidence.
- Prior review and human approval evidence exists, or the issue is routed to
  `Need Human Input` instead of merged.

## Merge-Lane Rework

For `Rework` caused by failed merging:

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
5. Delete the remote branch only when safe and after the merge succeeds.
6. Record merge evidence and reconcile issue/Project state to `Done`.

If `mergeStateStatus` is `UNKNOWN`, wait briefly and re-run the same `gh pr view`
query before making a routing decision. Only merge after the status returns
`CLEAN`.

## Status Transition Ordering

Project `Status` changes must be the final mutating step of the session. Before
moving an issue to `Done`, `Need Human Input`, or another routing state, finish
merge evidence, PR/issue reconciliation, workpad comments, and safe branch
cleanup. After status changes, do only readback verification such as
`project-issue`, `project-state`, or `doctor`.

## Hard Boundaries

- Never claim fresh `Todo` implementation.
- Never use the `Main Agent` field.
- Never create a new feature branch for merge-lane work unless the existing
  branch is unrecoverable and the operator explicitly agrees.
- Never merge without approval evidence.
- Never hide unknown mergeability, missing PR linkage, or missing context.
- Never mark `Human Review` yourself as a substitute for actual review approval.
