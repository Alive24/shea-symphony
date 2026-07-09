---
name: shea-symphony-manual-review
description: Use when manually reviewing a Shea Symphony GitHub issue or pull request as a Review Agent, while recording evidence in the Shea Symphony tracker without confusing manual review with automatic review loop evidence.
metadata:
  short-description: Shea Symphony manual review
  suite-version: 2026.05.22
---

# Shea Symphony Manual Review

Use this skill for an independent manual Review Agent pass on a Shea Symphony
issue or PR, especially when automatic `review loop` is blocked, timed out, or
needs a human-supervised pass.

## Runtime Topology

Live Shea Symphony review work still runs through the protected 2606 MVP
runtime until 2607 replaces the runtime spine. Use the MVP worktree for CLI/App
execution and the active Shea Symphony development worktree only as source
context.

MVP runtime worktree:

```bash
/Users/chuntengxiao/.shea-symphony/mvp/shea-symphony-2606-mvp
```

Canonical workflow inside the MVP runtime:

```bash
workflows/shea-symphony.md
```

GitHub repository:

```text
Alive24/shea-symphony
```

Run Shea Symphony CLI read/write commands from the MVP runtime only. Do not
change the MVP runtime branch or inspect PR code there; use the issue worktree
for implementation/review inspection.

If the Tauri App is needed, start it from the MVP runtime `app/` directory with
the local MVP profile:

```bash
cd /Users/chuntengxiao/.shea-symphony/mvp/shea-symphony-2606-mvp/app
SHEA_SYMPHONY_APP_PROFILE_PATH=/Users/chuntengxiao/.shea-symphony/mvp/app-profile-shea-symphony.json npm run tauri -- dev
```

The profile points at the Shea Symphony target checkout and the MVP CLI binary.
Do not assume `npm run tauri -- dev -- --workdir <path>` alone keeps the backend
on MVP code.

## Core Rule

Manual review evidence is not automatic `review loop` evidence.

Before reviewing, claim the tracker `Review Agent` field so parallel reviewers
do not work on the same issue. `Review Agent` is a Project text field. Use Shea
Symphony CLI review commands to write the structured text claim; do not use
legacy labels such as `Gemini A` or `Manual Gemini A`.

When you finish, save the manual note section headed exactly:

```md
## Manual Agent Review Evidence
```

`review pass` or `review reject` wraps that note in a standalone
`## Shea Symphony Agent Review Run` timeline comment. Do not claim that
`review loop` passed unless `shea-symphony review loop` itself produced that
result.

## Workflow

1. Identify the issue number and PR number.
2. Read issue and PR metadata with `gh issue view`, `gh pr view`, and
   `cargo run -- project issue workflows/shea-symphony.md '#<issue>' --json`.
3. Confirm the PR closes or clearly links to the issue.
4. Confirm the issue is in `Agent Review`, unless the operator explicitly asks
   for re-review.
5. Claim the `Review Agent` text field with `review claim`.
6. Discover the existing issue workspace:

```bash
cargo run -- workspace show workflows/shea-symphony.md '#<issue>'
```

Reuse the Main Agent issue worktree for local inspection and verification. If no
worktree can be found and the CLI does not expose a safe workspace ensure
command, stop and ask the operator for the intended workspace.

7. Review the PR using the available review extension or direct diff inspection.
8. Classify the result conservatively.
9. Evaluate the issue body checklists under `Expected Outcome`,
   `Completion Criteria`, `Functional Verification`, and `Context Verification`.
   Treat `UAT` as Human Review-owned: assess whether UAT instructions and
   operator evidence are sufficient, but do not check UAT boxes yourself.
10. If the review passes, update the issue body in place so only
    evidence-backed, non-UAT satisfied items are checked. Leave unsupported,
    skipped, failed, and all UAT items unchecked.
11. Save the review evidence to a local evidence file.
12. Route the result with `review pass` or `review reject`. For routine native
    subissues, `review pass` routes to `Merging`, not `Human Review`; parent
    final issues and ordinary issues still route to `Human Review`. Direct
    subissue Human Review requires `Subissue Human Review Exception: <reason>`.

Status transition ordering: `review pass` or `review reject` must be the final
mutating step of the manual review session. After the status changes, do only
readback verification such as `project issue` or `doctor`.

## Review Agent Claim

Claim with:

```bash
cargo run -- review claim workflows/shea-symphony.md '#<issue>' \
  --worker "manual-review-issue-<issue>" \
  --write
```

Copy the exact printed claim value into the evidence file as
`Review Agent claim`. If the claim fails or the issue is not in `Agent Review`,
stop and report that review cannot be claimed safely.

## Claim Finalization

After producing the evidence file:

```bash
cargo run -- review pass workflows/shea-symphony.md '#<issue>' \
  --evidence-file /path/to/manual-review-evidence.md \
  --write
```

```bash
cargo run -- review reject workflows/shea-symphony.md '#<issue>' \
  --target-state rework \
  --evidence-file /path/to/manual-review-evidence.md \
  --write
```

Current Shea Symphony CLI owns terminal claim cleanup during `review pass` and
`review reject`. Do not run `review-clear-claim` unless the operator explicitly
asks you to release an abandoned or mistaken active claim.

## Evidence Template

`review pass` and `review reject` write this evidence as a standalone
append-only `Shea Symphony Agent Review Run` timeline comment. Do not edit,
overwrite, or restructure the Main Agent Workpad.

```md
## Manual Agent Review Evidence

- Issue: #<issue>
- PR: #<pr> <url>
- Lane: `review`
- Actor role: `review_agent`
- Run ID: `<run-id from Review Agent claim>`
- Input state: `Agent Review`
- Target state after review routing: Human Review / Rework / Agent Review / Need Human Input
- Result: ManualPass / ManualRework / ManualInconclusive / ManualInfrastructureBlocked
- Reviewer: manual Review Agent
- Review mode: Manual
- Review Agent claim: `<exact value printed by review claim>`
- Review workspace: `<issue worktree path>` / `not inspected locally`
- Classification: ManualPass / ManualRework / ManualInconclusive / ManualInfrastructureBlocked
- Recommended tracker state: Human Review / Rework / Agent Review / Need Human Input

### Summary

...

### Findings

- ...

### Issue Body Checklist Review

- Expected Outcome: checked / unchecked / not applicable, with evidence.
- Completion Criteria: checked / unchecked / not applicable, with evidence.
- Functional Verification: checked / unchecked / not applicable, with evidence.
- UAT: Human Review-owned; leave unchecked and note pending / operator evidence / not applicable.
- Context Verification: checked / unchecked / not applicable, with evidence.

### Evidence Boundary

This is manual/operator-supplied Review Agent evidence. It is not automatic
`review loop` pass evidence.
```

## Safety

- Do not merge PRs.
- Do not force-push.
- Do not edit implementation code.
- Do not run `gh pr checkout`, `git checkout`, or `git switch` in the canonical
  checkout.
- Do not change the canonical checkout away from `main`.
- Do not start review while another active `Review Agent` claim exists.
- Do not use legacy single-select values.
- Do not manually clear terminal review claims; let routing commands preserve
  audit evidence.
- Do not revise a `Human Review` issue by raw Project mutation or
  `forge promote`. If the reviewed contract must change, hand the operator
  decision to Issue Forge and use the deterministic `forge rework` flow with a
  replacement body, evidence file, and explicit confirmation.
- Do not check issue body checklist items unless PR diff, Main Agent Workpad,
  timeline comment evidence, command output, or operator evidence supports them.
- Do not check `UAT` checklist items.
