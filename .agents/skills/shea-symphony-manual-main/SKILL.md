---
name: shea-symphony-manual-main
description: Use when the operator wants this agent to execute one Shea Symphony Main-lane issue now, including Todo implementation, Main-lane Rework, or resumable In Progress work. Perform the claim, isolated worktree work, verification, PR publication, tracker evidence, and Agent Review handoff in the current task. Do not use this skill merely to write a prompt or delegate Main work to another task.
metadata:
  short-description: Execute one manual Shea Symphony Main lane
  suite-version: 2026.08.07
---

# Shea Symphony Manual Main Agent

Execute one operator-selected Main issue in the current task. This is an
operational skill, not a prompt generator: inspect the real tracker, mutate the
selected issue only after its gates pass, edit the issue worktree, verify the
change, publish a ready PR, and stop at `Agent Review`.

Do not create another task, produce a handoff prompt, or invoke a configured
agent backend in place of doing the work. Use another lane skill only when the
selected issue is genuinely outside Main authority.

## Authority

Main owns:

- `Todo` implementation;
- Main-lane `Rework` caused by Agent Review findings or a Human Review contract
  revision;
- clearly resumable `In Progress` work already claimed by this Main worker;
- recovery of the issue worktree, branch, commit, PR, and workpad when that
  recovery is within the accepted issue scope.

Main does not own:

- Backlog shaping or promotion;
- independent review, UAT approval, or Human Review decisions;
- `Merging`, merge-lane `Rework`, or merging a PR.

Use `$shea-symphony-issue-forge` for Backlog promotion,
`$shea-symphony-manual-review` for review, and
`$shea-symphony-manual-merge` for merge-lane work. A Backlog issue is not an
implementation instruction: report that it must be promoted and stop without
claiming or editing code.

## Bind the Active Repository

Never depend on hard-coded user names, volumes, checkout paths, or a historical
MVP worktree. From the target repository root:

1. Read the profile selected by `SHEA_SYMPHONY_APP_PROFILE_PATH`, when set.
   Otherwise prefer `.shea/app-profile.local.json` over
   `.shea/app-profile.json`. Use it for `workflow_path` and `cli_path`.
2. Otherwise prefer `.shea/workflows/shea-symphony.md` and
   `.shea/bin/shea-symphony` when they exist.
3. Resolve both paths to absolute paths and verify the CLI with `--help` before
   any mutation.
4. Read the workflow's repository, Project, workspace root, and
   `git.base_branch`. Also read any explicit target or backport branch in the
   issue contract. An explicit issue target wins over a generic workflow
   default and must be confirmed before creating a branch or PR.

Use concise shell variables in subsequent commands, for example:

```bash
SHEA_CLI="<resolved-cli-path>"
SHEA_WORKFLOW="<resolved-workflow-path>"
ISSUE="#<number>"
```

Do not substitute `cargo run` without checking what the current checkout
builds. In the current 2607 topology, plain `cargo run` starts the Temporal
worker and is not the protected 2606 operational CLI. Use the resolved vendored
CLI when the profile selects it.

## Quota-Safe Preflight

Use targeted reads for the operator-selected issue:

```bash
"$SHEA_CLI" project issue "$SHEA_WORKFLOW" "$ISSUE" --json
"$SHEA_CLI" project inspect "$SHEA_WORKFLOW" "$ISSUE" --lane main
"$SHEA_CLI" forge validate --workflow "$SHEA_WORKFLOW" --issue "$ISSUE"
"$SHEA_CLI" workspace show "$SHEA_WORKFLOW" "$ISSUE"
```

Raw `gh issue view` and `gh pr view` are acceptable for ordinary issue and PR
content. Shea's targeted Project read surface remains authoritative for status,
claims, dependencies, native subissues, and linked PRs.

Do not make `project state`, `autopilot plan`, a global `doctor`, or an
all-lane loop part of routine Manual Main preflight. Those commands scan the
whole Project and can consume disproportionate GraphQL quota. Use one only when
its global information is necessary to resolve a concrete ambiguity. If GitHub
reports exhausted or nearly exhausted GraphQL quota, record the limit/reset
evidence and stop instead of retrying scans.

## Selection Gates

Proceed only when all are true:

- status is `Todo`, Main-lane `Rework`, or matching/resumable `In Progress`;
- the `Main Agent` field is empty or identifies this worker/session;
- native `blocked by` relationships are terminal or explicitly non-blocking;
- every native subissue of a parent issue has Project status `Done`;
- Issue Quality Gate is `Ready` or `ReadyWithAssumptions`;
- the issue contract is implementable without inventing product decisions;
- the target base branch and existing workspace/PR identity are unambiguous.

An issue being closed is not proof that its Project dependency or subissue gate
is terminal. For Main-lane `Rework` created by `forge rework`, a missing linked
PR or missing worktree record is recoverable evidence, not automatically a
claim blocker.

If the contract is incomplete, record evidence and route or recommend
`Need to Clarify`. If work requires credentials, samples, external authority,
or a product decision, route or recommend `Need Human Input`. Do not claim first
and ask basic scope questions afterward.

## Claim and Workspace

After all read-only gates pass:

1. Choose a stable worker identity for this task.
2. Claim through the supported CLI, not raw Project GraphQL:

   ```bash
   "$SHEA_CLI" main claim "$SHEA_WORKFLOW" "$ISSUE" \
     --worker "<worker-id>" --source manual --write
   ```

3. Read the issue back and confirm the claim and `In Progress` status.
4. Reuse the single canonical issue worktree and PR branch when evidence is
   consistent. If none exists, create one isolated worktree and feature branch
   from the confirmed target base branch. Never implement in the canonical
   checkout.
5. Inspect `git worktree list --porcelain`, the existing Main Workpad, linked PR
   evidence, and branch state before adopting or creating anything. Stop for
   operator choice when multiple strong candidates disagree.

The configured workspace root controls where a new worktree belongs. One issue
has one implementation branch, one canonical worktree, and one PR. Do not push
unrelated canonical-checkout changes into the issue branch.

## Execute the Main Loop

Perform the work yourself:

1. Read the issue body, canonical Main Workpad, append-only timeline comments,
   relevant repository instructions, authoritative docs, and current code.
2. Update the one canonical Main Workpad with an issue-specific checkbox plan
   before implementation.
3. Implement only the accepted scope in the issue worktree.
4. Add focused tests and documentation required by the issue. Add comments only
   for non-obvious runtime, tracker, schema, retry/idempotency, compatibility,
   or external-service boundaries.
5. Run the strongest practical repository-owned verification for the touched
   area. Repair in-scope lint, formatting, type, build, or test failures and
   repeat verification; do not declare completion while a repairable in-scope
   check fails.
6. For changed Rust public API, also run
   `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`, add semantic Rustdoc, audit
   whether each item must remain public, and use only narrowly justified
   `#[allow(missing_docs)]` annotations.
7. Update the workpad with changed files, verification commands and results,
   risks, follow-ups, and compatibility/comment/Rustdoc evidence.
8. Commit and push the issue branch. Open or update one PR against the confirmed
   target base, include `Closes #<issue>` when merge should close it, and make
   the PR ready for review rather than draft.
9. If necessary, record PR linkage through the supported CLI:

   ```bash
   "$SHEA_CLI" project link-pr "$SHEA_WORKFLOW" "$ISSUE" "#<pr>" --write
   ```

10. Read back the issue and PR. Confirm that Shea exposes the linked PR and that
    GitHub reports it ready and not draft.
11. Complete the workpad with the PR URL, linked-PR confirmation, verification,
    and why Main stops at `Agent Review`.
12. Move to `Agent Review` only as the final mutation, then perform read-only
    verification:

    ```bash
    "$SHEA_CLI" project set-state "$SHEA_WORKFLOW" "$ISSUE" agent_review --write
    "$SHEA_CLI" project issue "$SHEA_WORKFLOW" "$ISSUE" --json
    ```

Use the CLI's workpad command to upsert the canonical workpad from a local
Markdown file whenever the skill performs a workpad write:

```bash
"$SHEA_CLI" project workpad "$SHEA_WORKFLOW" "$ISSUE" "<workpad.md>" --write
```

## Workpad Contract

Maintain exactly one `Shea Symphony Main Agent Workpad` in place. It contains:

- `### Plan` with issue-specific checkboxes;
- `### Work Log` with timestamped progress;
- scope boundary and changed files;
- verification commands and results;
- boundary-comment and Rustdoc/public-visibility audit, or `not applicable`;
- branch, worktree, commit, PR URL, ready/not-draft state, and linked-PR readback;
- final handoff summary.

For Main-lane Rework, add the new round to this workpad instead of creating a
second one. Other lane run comments remain append-only and must not be folded
into or overwritten by Main. The workpad is evidence; it does not replace the
unchecked Review, completion, functional verification, UAT, or context
checklists in the issue body.

## Mutation Ordering and Hard Boundaries

Project `Status` is the final mutation of each state-changing phase. Finish the
claim, workspace/PR updates, workpad write, PR readiness check, linkage check,
and supporting evidence before the status transition. Afterward, perform only
readback verification.

- Never implement a Backlog issue.
- Never bypass issue quality, dependency, subissue, or target-branch gates.
- Never move an issue to `Human Review`.
- Never merge a PR or use the `Merging Agent` field.
- Never turn merge-lane repair into Main work.
- Never hide quota, usage-limit, trust, permission, or backend failures.
- Never finish by merely giving the operator a prompt for another agent.
