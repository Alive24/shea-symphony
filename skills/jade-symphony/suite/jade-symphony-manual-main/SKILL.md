---
name: jade-symphony-manual-main
description: Use when manually running a Codex Main Agent session for Jade Symphony implementation or Main-lane Rework from a fresh Codex session. This skill claims Todo, Main-lane Rework, or resumable In Progress work through the Main Agent lane, preserves issue quality and dependency gates, creates or resumes isolated workspaces and PRs, and hands off only to Agent Review.
metadata:
  short-description: Jade Symphony manual Main Agent
  suite-version: 2026.05.23
---

# Jade Symphony Manual Main Agent

Use this skill to operate a human-supervised Jade Symphony Main Agent session.
The Main Agent owns implementation work. It does not own review approval, human
approval, or merging.

For normal all-lane dogfood, prefer the CLI foreground path first:

```bash
cargo run -- autopilot plan workflows/jade-symphony.md
cargo run -- autopilot loop workflows/jade-symphony.md --max-iterations 1 --write
```

Use this manual skill only for operator-selected Main implementation, Main-lane
`Rework`, focused debugging, or break-glass recovery. Do not replace normal
long-running dogfood with three separate manual lane loops.

## Repository

Default repository:

```bash
cd /Volumes/Bohemialive/GitHub/jade-symphony
```

Canonical workflow:

```bash
workflows/jade-symphony.md
```

Canonical Main Agent prompt:

```bash
workflows/prompts/main-agent.md
```

## Operating Rule

Before doing any work:

1. Refresh tracker state from GitHub Project v2 and local runtime state.
2. Respect the `Main Agent` Project field as the claim lock.
3. Treat native Project relationships such as `blocked by` as dependency gates.
4. Run the issue quality gate before implementation.
5. Use one isolated worktree, one branch, and one PR per issue.

Handle only:

- `Todo` issues that pass the issue quality gate and dependency checks.
- `Rework` issues that are Main-lane repair work after Agent Review or
  Human Review contract revision, once dependencies and issue quality pass.
- `In Progress` issues already claimed by this Main Agent session or clearly
  resumable from prior interrupted Main Agent work.

Parent issues with native GitHub subissues are not claimable just because they
are `Todo` or Main-lane `Rework`. Treat the native subissue set as dynamic and
require every native subissue to have Project status `Done` before selecting or
claiming the parent. A GitHub issue `closed` state is not enough for this gate.

Do not use this skill for merge-lane `Rework` or `Merging` work. Use
`$jade-symphony-manual-merge` for those. When `Rework` came from
`forge rework`, missing linked PR or missing local worktree evidence is not a
claim blocker; the Main Agent owns PR/workspace recovery inside the issue
scope.

## Preflight

Run or equivalent-check:

```bash
cargo run -- project state workflows/jade-symphony.md
cargo run -- autopilot plan workflows/jade-symphony.md
cargo run -- doctor workflows/jade-symphony.md
cargo run -- project inspect workflows/jade-symphony.md '#<issue>'
cargo run -- project issue workflows/jade-symphony.md '#<issue>' --json
cargo run -- forge validate --workflow workflows/jade-symphony.md --issue '#<issue>'
```

Use the Jade Symphony CLI Project read surface instead of raw Project GraphQL.
Raw `gh issue view` and `gh pr view` are acceptable for ordinary issue and PR
content. Raw Project field/status/claim reads or mutations are break-glass only;
record the reason if they are needed.

If `autopilot plan` shows the same Main issue as the next lane action and there
is no need to isolate Main, run `autopilot loop --write` from clean canonical
`main` instead of starting a manual Main session.

## Selection

Pick the issue only when all are true:

- Status is `Todo`, Main-lane `Rework`, or `In Progress` with
  matching/resumable Main Agent evidence.
- If status is `Rework`, the trigger must be Agent Review findings or Human
  Review contract revision that requires Main implementation repair. Merge-lane
  conflicts, stale-base repair, Merging failures, or any issue with an active
  `Merging Agent` claim are not Main-lane Rework.
- `Main Agent` field is empty or belongs to this session.
- Dependency relationships are terminal or explicitly non-blocking.
- If it is a native parent issue, every native GitHub subissue has Project
  status `Done`.
- Issue Quality Gate is `Ready` or `ReadyWithAssumptions`.
- The issue body has enough context to implement without inventing product
  decisions.

If the issue contract is incomplete, route or recommend routing to
`Need to Clarify` with evidence. If implementation needs external human input,
credentials, product decisions, or missing samples, route or recommend routing
to `Need Human Input` with evidence.

## Implementation Loop

For the selected issue:

1. Claim through the `Main Agent` field and transition to `In Progress`.
2. Create or resume the isolated worktree and feature branch.
3. Read the issue body, Main Agent Workpad, append-only timeline comments,
   canonical docs, and relevant code.
4. Implement only the accepted issue scope.
5. Run the strongest practical verification for the touched area.
6. Update issue or PR evidence with changes, verification, risks, and follow-ups.
7. Open or update the PR.
8. Verify the issue Project item exposes the PR under linked pull requests.
9. Confirm the PR is ready for review, not draft.
10. Move the issue to `Agent Review`.

The Main Agent must stop at `Agent Review`. Draft PRs must not be handed off.

## Status Transition Ordering

Project `Status` changes must be the final mutating step of each state-changing
session phase. Before moving an issue to `In Progress`, `Need to Clarify`,
`Need Human Input`, or `Agent Review`, finish every required claim,
worktree/PR update, workpad write, PR readiness check, linked-PR verification,
and evidence update that justifies that state. After the status changes, do only
readback verification such as `project issue` or `doctor`.

## Main Agent Workpad Evidence

Keep exactly one durable Jade Symphony workpad updated in place. It must include:

- `### Plan` before implementation, as issue-specific checkboxes for reading,
  implementation, verification, PR readiness, and Agent Review handoff.
- `### Work Log` with timestamped progress notes.
- changed files and scope boundary.
- verification commands and results.
- PR URL, linked-PR confirmation, and ready/not-draft status.
- final handoff summary explaining why Main stops at `Agent Review`.

Main-lane `Rework` is still Main implementation work. If the issue was returned
from Agent Review or revised from Human Review, update this same Main Agent
Workpad with the new rework round, current plan/work log, changed files,
verification, PR readiness, and handoff evidence. Do not create a second
canonical Main Workpad for Main-lane rework.

Standalone `Jade Symphony Rework Run` comments are append-only trigger or
diagnostic records explaining why the issue entered `Rework`; they are not the
current-state implementation evidence surface. Review, Merge, Human Review, and
Doctor runs write their own append-only Jade Symphony timeline comments and
must not overwrite, restructure, or fold their run logs into the Main Agent
Workpad.

Do not treat the workpad as a replacement for the issue body's Review checklist.
The issue body should retain unchecked `Expected Outcome`, `Completion Criteria`,
`Functional Verification`, `UAT`, and `Context Verification` items for the
independent Review Agent to evaluate and check.

## PR Linkage Check

Before handoff, do not rely on a workpad comment or `project link-pr` output alone.
Confirm the CLI Project read surface exposes the PR under linked pull requests:

```bash
cargo run -- project issue workflows/jade-symphony.md '#<issue>' --json
gh pr view <pr-number> --repo Alive24/jade-symphony --json number,isDraft,url
```

Prefer a GitHub closing keyword such as `Closes #<issue>` in the PR body when
the PR is intended to close the issue after merge.

## Hard Boundaries

- Never move an issue to `Human Review`.
- Never merge a PR.
- Never use the `Merging Agent` field.
- Never bypass the issue quality gate.
- Never continue when a dependency relationship blocks the issue.
- Never convert merge-lane rework into a new implementation issue unless the
  operator explicitly asks.
- Never hide usage-limit, trust, permission, or backend failures; record
  evidence and stop or route state conservatively.
