---
name: shea-symphony-manual-main
description: Use when the operator wants this agent to execute one Shea Symphony Main-lane issue now, including Todo implementation, explicitly authorized Backlog pickup without promotion, Main-lane Rework, or resumable In Progress work. Perform the isolated worktree work, verification, PR publication, tracker evidence, and Agent Review handoff in the current task. Do not use this skill merely to write a prompt or delegate Main work to another task.
metadata:
  short-description: Execute one manual Shea Symphony Main lane
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
- an operator-named Backlog issue when the operator explicitly says to execute
  it without promotion and its body passes the Todo-grade contract gate;
- Main-lane `Rework` caused by Agent Review findings or a Human Review contract
  revision;
- clearly resumable `In Progress` work already claimed by this Main worker;
- recovery of the issue worktree, branch, commit, PR, and workpad when that
  recovery is within the accepted issue scope.

Main does not own:

- shaping, promoting, or selecting an ordinary Backlog item without explicit
  operator execution authority;
- independent review, UAT approval, or Human Review decisions;
- `Merging`, merge-lane `Rework`, or merging a PR.

Use `$shea-symphony-issue-forge` for ordinary Backlog shaping or promotion,
`$shea-symphony-manual-review` for review, and
`$shea-symphony-manual-merge` for merge-lane work. A Backlog issue is not an
implementation instruction by itself. Execute it only through the explicit
operator-confirmed fast path below.

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

- status is `Todo`, Main-lane `Rework`, matching/resumable `In Progress`, or an
  explicitly operator-authorized Backlog pickup;
- the `Main Agent` field is empty or identifies this worker/session;
- native `blocked by` relationships are terminal or explicitly non-blocking;
- every native subissue of a parent issue has Project status `Done`;
- Issue Quality Gate is `Ready` or `ReadyWithAssumptions`;
- the issue contract is implementable without inventing product decisions;
- the target base branch and existing workspace/PR identity are unambiguous.

For a Backlog pickup, additionally validate the current title and body as a
Todo-grade draft without changing tracker state:

```bash
"$SHEA_CLI" forge validate --workflow "$SHEA_WORKFLOW" --status todo \
  --title "<current-title>" --body-file "<current-body.md>"
```

Do not use the relaxed Backlog validation result as proof that the issue is
implementation-ready.

An issue being closed is not proof that its Project dependency or subissue gate
is terminal. For Main-lane `Rework` created by `forge rework`, a missing linked
PR or missing worktree record is recoverable evidence, not automatically a
claim blocker.

If the contract is incomplete, record evidence and route or recommend
`Need to Clarify`. If work requires credentials, samples, external authority,
or a product decision, route or recommend `Need Human Input`. Do not claim first
and ask basic scope questions afterward.

## Claim and Workspace

After all read-only gates pass, use the normal claim path for `Todo`, `Rework`,
or resumable `In Progress`:

1. Choose a stable worker identity for this task.
2. Inspect the current task workspace, existing Main Workpad, linked PR
   evidence, session/runtime ownership, branch state, and `git worktree list
   --porcelain` before claiming. Reuse the single canonical issue worktree when
   evidence is consistent; otherwise evaluate the current task worktree for
   adoption before creating anything.
3. Record the selected workspace through `workspace adopt --write` and verify
   `workspace show` exposes it as the single canonical candidate. The current
   CLI uses that exact worktree for runtime readiness and refuses a live Main
   claim without adoption evidence.
4. Claim through the supported CLI, not raw Project GraphQL:

   ```bash
   "$SHEA_CLI" main claim "$SHEA_WORKFLOW" "$ISSUE" \
     --worker "<worker-id>" --source manual --write
   ```

5. Read the issue back and confirm the claim. Record successful runtime
   readiness and ownership evidence in the canonical workpad, then make Project
   Status the phase's final mutation with `project set-state ... in_progress
   --write` and verify the readback.
6. Create a new isolated worktree and feature branch from the confirmed target
   base only when neither an existing canonical issue worktree nor the current
   task worktree is safe to use. Never implement in the canonical checkout.

### Current-task worktree adoption

Codex App and other operator harnesses may start this task in an isolated git
worktree. Treat that worktree as a reuse candidate, not as a parent directory
in which Shea should automatically create another worktree.

1. Resolve the current top-level path and common git directory, then correlate
   them with `git worktree list --porcelain` and the target repository.
2. Adopt the current worktree only when it is a registered worktree for the
   target repository, is not the canonical checkout, has no active git
   operation, and has no ownership or issue evidence that conflicts with the
   selected issue. For new work, require a clean tracked and untracked status.
   For resume, allow existing changes only when the issue branch, claim,
   workpad, and PR evidence consistently identify them as this issue's work.
3. If the current worktree already uses the one intended issue branch, keep it.
   For new work in a clean detached worktree, first prove that `HEAD` has no
   unique work relative to the confirmed target base, then create or switch to
   the intended issue branch from that base only after verifying the branch is
   not checked out elsewhere. Preserve detached commits only through the resume
   path when issue evidence identifies them. Do not repurpose a branch owned by
   another issue.
4. Record the chosen path through the supported workspace surface:

   ```bash
   "$SHEA_CLI" workspace adopt "$SHEA_WORKFLOW" "$ISSUE" \
     "<current-worktree-path>" --write
   "$SHEA_CLI" workspace show "$SHEA_WORKFLOW" "$ISSUE"
   ```

5. Continue only when readback exposes that path as the single canonical issue
   workspace. Adoption transfers issue-workspace evidence, not cleanup
   ownership: an external harness remains responsible for removing its own
   task worktree.

Stop for operator choice when multiple strong candidates disagree, the current
branch or changes belong to another task, or the target base cannot be proven.
The configured workspace root controls genuinely new Shea-owned worktrees, but
if that root resolves inside an already isolated current task worktree, do not
create a nested worktree there. Adopt the current worktree or stop and resolve
the workspace root. One issue has one implementation branch, one canonical
worktree, and one PR. Do not push unrelated canonical-checkout changes into the
issue branch.

### Operator-confirmed Backlog fast path

Use this only when the operator names the issue and explicitly chooses direct
Manual Main execution without promotion:

1. Keep Project Status `Backlog` during implementation so unattended Main does
   not compete for the issue.
2. Do not call `main claim` and do not move the issue to `In Progress`; those
   commands implement the normal dispatchable-state path.
3. Confirm there is no existing Main claim, conflicting worktree, branch, or PR.
4. Record the operator confirmation, Todo-grade validation result, and the
   intentional skipped states in the canonical Main Workpad.
5. Follow the same isolated worktree, implementation, verification, ready PR,
   linked-PR, and evidence requirements as normal Main.
6. If work cannot reach a ready PR, leave the issue in Backlog and record the
   blocker. Do not manufacture a partial handoff.
7. Once every Main handoff gate passes, move directly from Backlog to
   `Agent Review` as the final mutation and verify the readback. This is the
   only status skip authorized by this fast path.

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
8. Commit and push the issue branch. Resolve the repository's default branch,
   then open or update one ready PR against the confirmed target base:

   - When the target base is the default branch, include `Closes #<issue>` in
     the PR body.
   - When the target base is not the default branch, do not rely on a closing
     keyword: GitHub ignores it for native issue linkage. Use a non-closing
     reference such as `Refs #<issue>` and follow the non-default-base linkage
     rule below.

9. Read back the issue and PR. Confirm that GitHub reports the PR ready and not
   draft, and inspect the exact linked-PR `source` exposed by Shea:

   - For a default-base PR, require the exact PR with `source=github_native`.
     Repair an incorrect or missing closing reference in the PR body and read
     back again. Do not hand off while native linkage is missing.
   - For a non-default-base PR, prefer an operator-created native link through
     GitHub's Development sidebar. If the issue contract or operator explicitly
     accepts diagnostic fallback evidence for this backport/protected-branch
     run, record it through the supported CLI:

   ```bash
   "$SHEA_CLI" project link-pr "$SHEA_WORKFLOW" "$ISSUE" "#<pr>" --write
   ```

   This command may report that native readback is missing after recording the
   diagnostic comment. Do not retry it. Perform one targeted issue readback,
   record `source=fallback_diagnostic` exactly, and never describe it as
   GitHub-native or as verified native linkage. Without explicit fallback
   acceptance, stop for the operator to create the native Development link.
10. Complete the workpad with the PR URL, target/default base, exact linked-PR
    source, any authorized fallback exception, verification, and why Main stops
    at `Agent Review`.
11. Move to `Agent Review` only as the final mutation, then perform read-only
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
- branch, worktree, workspace origin (`reused`, `current-task adopted`, or
  `Shea-created`), adoption readback when applicable, commit, PR URL,
  ready/not-draft state, and linked-PR readback;
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

- Never implement a Backlog issue without explicit operator execution authority
  and a passing Todo-grade contract validation.
- Never bypass issue quality, dependency, subissue, or target-branch gates.
- Never create a nested issue worktree before evaluating a safe current task
  worktree for adoption.
- Never treat `fallback_diagnostic` PR evidence as GitHub-native linkage or use
  it for a default-base PR handoff.
- Never move an issue to `Human Review`.
- Never merge a PR or use the `Merging Agent` field.
- Never turn merge-lane repair into Main work.
- Never hide quota, usage-limit, trust, permission, or backend failures.
- Never finish by merely giving the operator a prompt for another agent.
