---
name: jade-symphony-human-review
description: Use when briefing a Jade Symphony operator for Human Review after independent Review Agent pass evidence, guiding UAT, recording a structured decision note, and routing only after explicit operator confirmation.
metadata:
  short-description: Jade Symphony Human Review briefing
  suite-version: 2026.05.22
---

# Jade Symphony Human Review

Use this skill when the operator wants help reviewing a Jade Symphony issue that
has passed independent Review Agent checks and is waiting for Human Review.

Human Review is the operator-owned final acceptance checkpoint before merge-lane
work. It is not implementation work, it is not the independent Review Agent, and
it is not merge execution.

## Repository

Default repository:

```text
Alive24/jade-symphony
```

Default local checkout:

```bash
/Volumes/Bohemialive/GitHub/jade-symphony
```

Canonical workflow:

```bash
workflows/jade-symphony.md
```

Canonical decision note template:

```bash
workflows/template/workpad/human-review.md
```

## Core Boundary

- Do not modify implementation code, except for the narrow PR branch freshness
  repair described below when the fix is mechanical and low-risk.
- Do not act as the independent Review Agent.
- Do not merge PRs or act as the Merging Agent.
- Do not move accepted work directly to `Done`.
- Accepted Human Review routes to `Merging`.
- Treat UAT checklist items as Human Review-owned unless the issue explicitly
  says otherwise.
- Native GitHub subissues are not routine Human Review surfaces. If invoked on a
  native subissue without `Subissue Human Review Exception: <reason>` evidence,
  stop before UAT and explain that passing subissue Agent Review should route
  directly to `Merging`; the parent issue owns final Human Review and UAT.
- Never mutate Project state until the operator explicitly confirms the decision
  after the briefing and UAT discussion.
- Use Jade Symphony CLI for Project reads and confirmed state routing. Do not
  bypass it with raw Project mutations.
- Human Review decision notes are append-only timeline evidence. They must not
  overwrite or restructure the canonical Main Agent Workpad.

## Conversation Language

- Match the operator-facing language to the current session's user language.
- Do not force English for Human Review briefings, UAT guidance, summaries, or
  confirmation prompts when the operator is using another language.
- Preserve exact command names, state names, file paths, issue titles, and
  decision labels in their canonical English form.
- Use English inside durable decision-note fields when the template, command
  surface, or issue evidence expects canonical values, but keep explanatory
  prose in the operator's session language.

## CLI Topology Transition

Issue #284 is cleaning up Jade Symphony CLI topology. Prefer the intended grouped
language in explanations:

- `project state`
- `project issue`
- `project set-state`
- `project timeline-comment`

Do not use `project workpad` for Human Review decision notes. That command
upserts the canonical Main Agent Workpad marker comment and is reserved for
Main implementation evidence, including Main-lane Rework rounds.
Use `project timeline-comment` for append-only Human Review decision notes.

During live use, if the current binary still exposes flat commands, use those
commands and say so in the decision note:

```bash
cargo run -- project state workflows/jade-symphony.md
cargo run -- project issue workflows/jade-symphony.md '#<issue>' --json
cargo run -- project timeline-comment workflows/jade-symphony.md '#<issue>' /path/to/human-review-note.md --write
cargo run -- project set-state workflows/jade-symphony.md '#<issue>' merging --write
```

Do not turn the topology transition into custom GitHub Project mutations.
Project status changes must still go through `project set-state`, after the
timeline comment has been written.

## Required Reads

Before briefing the operator, read the decision surfaces:

```bash
cargo run -- project issue workflows/jade-symphony.md '#<issue>' --json
gh issue view <issue> --repo Alive24/jade-symphony --comments
gh pr view <pr> --repo Alive24/jade-symphony --json number,title,state,url,isDraft,baseRefName,headRefName,mergeStateStatus,reviewDecision,statusCheckRollup
```

Inspect the issue body and workpad for:

- issue goal, scope, guardrails, and dependencies;
- Expected Outcome, Completion Criteria, Functional Verification, UAT, and
  Context Verification checkboxes;
- Review Agent pass evidence and any explicitly unchecked review items;
- linked PR identity, readiness, base branch, and check/review state;
- missing evidence, stale assumptions, or blockers that should prevent approval.

Do not drown the operator in raw JSON. Summarize the decision-relevant facts and
include exact issue and PR references.

## PR Freshness Repair Gate

Before any PR-specific UAT, verify that the reviewed PR branch contains the
latest `origin/main`. Run this only from the linked PR/issue worktree, never
from the canonical `main` checkout.

Jade Symphony write-mode lane commands may now fast-forward the canonical
checkout before tracker mutation. That is separate control-surface
synchronization and is not evidence that the reviewed PR branch is fresh.

From the linked PR/issue worktree, run:

```bash
git fetch origin
git merge-base --is-ancestor origin/main HEAD
```

Interpret and repair:

- Exit code `0`: the PR branch contains latest `origin/main`; continue to UAT.
- Non-zero exit code: immediately attempt a safe local branch refresh:

```bash
git merge --no-edit origin/main
```

If the merge is clean, run targeted verification, push the PR branch, record the
refresh in the running Human Review note draft, then continue UAT.

If conflicts or failures are small, mechanical, and clearly caused by freshness
drift, resolve them in the PR worktree, run the relevant verification, commit,
push the PR branch, record the repair in the note draft, then continue UAT.

If conflicts are broad, product-scope, ambiguous, or verification fails in a way
that is not obviously mechanical, stop before UAT and recommend `Request Rework`
with the smallest actionable finding.

If the PR worktree cannot be found, do not run the freshness check from the
canonical `main` checkout. First select or create a PR branch worktree. If that
cannot be done safely, record the missing worktree as a UAT blocker.

If `gh pr view` reports a non-clean merge state, treat that as corroborating
freshness or mergeability risk. The local `merge-base` check is still required
before PR-specific UAT because GitHub mergeability can lag or be temporarily
unknown.

## Brief The Operator

Before any UAT command, freshness repair, decision-note drafting, or state
mutation, start with a plain-language orientation brief. The operator should be
able to understand what they are reviewing without opening GitHub first.

Give a concise Human Review brief with:

- issue and PR identity, including title, PR number, branch, base branch, and
  current Project state;
- one-sentence purpose: what problem this issue/PR was meant to solve;
- what the issue was supposed to deliver;
- what changed and where the PR is, summarized from the Main workpad, PR
  metadata, and review evidence rather than raw diffs;
- why this item is in Human Review now;
- what the Review Agent already checked;
- what remains human-owned, especially UAT;
- any missing evidence, stale assumption, or risk;
- available decisions and their target states.

Recommended opening shape:

```text
## Human Review Brief

Issue: #<issue> <title>
PR: #<pr> <title or URL>
State: Human Review

What this is about: <one-sentence issue purpose>
What changed: <2-4 bullets summarizing the PR in operator language>
Why you are reviewing it now: <Review passed / parent owns UAT / approval gate>
Review Agent already checked: <short evidence summary>
Human-owned part: <UAT or acceptance decision still needed>
Risks / things to watch: <none / concise list>
Available decisions: Approve for Merging / Request Rework / Need Human Input / Defer
```

For parent issues with native subissues, explicitly summarize the parent/child
shape: which child issues are Done, which child PRs landed, which parent PR is
being accepted, and what combined behavior the parent UAT is meant to validate.

If the issue is not in `Human Review`, or if Review Agent pass evidence or a
reliable linked PR is missing, stop before UAT and recommend the smallest safe
route such as `Need Human Input`, `Agent Review`, or no state change.

If the issue is a native subissue, check whether direct Human Review was
explicitly excepted. Without `Subissue Human Review Exception: <reason>`, do
not ask the operator for routine child approval; recommend returning the child
to the correct Review PASS -> `Merging` path and reviewing the parent issue for
final UAT.

## Interactive Guidance

After the briefing, guide the operator one step at a time.

- Do not dump the whole UAT checklist as a single todo list unless the operator
  explicitly asks for the full list.
- Give exactly one next action, explain why it is the next action, and tell the
  operator what feedback to provide after running it.
- Tell the operator which directory to run the action from. For PR UAT, this is
  normally the reviewed PR/issue worktree, not the canonical `main` checkout.
- Wait for the operator's result before moving to the next UAT action.
- After each operator result, classify it as `pass`, `fail`, `deferred`, or
  `needs clarification`, then choose the next action.
- Keep a running Human Review note draft in the conversation so the final
  decision note is assembled from actual operator feedback, not reconstructed
  from memory.
- If a command output is ambiguous, ask for the smallest missing fact instead of
  advancing the workflow.
- Only move from UAT guidance to decision confirmation after the required
  operator-owned checks have explicit pass/fail/deferred notes.

Recommended step format:

```text
Next action: <one command or inspection>
Why: <one sentence>
Please reply with: pass/fail/deferred plus the key output lines, or paste the error.
```

## Guide UAT

Walk the operator through UAT items from the issue body.

- Treat unchecked UAT items as human-owned.
- Ask for concrete pass/fail/deferred notes when useful.
- If UAT cannot be performed, record what is missing.
- If UAT fails, recommend `Rework` with the smallest actionable finding.
- Do not check UAT boxes based only on Review Agent evidence.

First resolve the correct execution directory.

- If the issue has an unmerged PR, UAT commands that validate the PR's code must
  run from the linked PR/issue worktree or another checkout of that PR branch.
- Do not ask the operator to run PR-specific UAT from the canonical `main`
  checkout unless the PR has already been merged into `main`.
- Before PR-specific UAT, apply the PR Freshness Repair Gate. Do not stop only
  because the PR branch is stale; first try the safe refresh/small-repair path.
- Prefer the worktree recorded in the issue workpad or `project issue` readback.
  If no usable worktree is available, ask the operator whether to create or
  select one before continuing.
- If the operator accidentally runs a UAT command from canonical `main`, classify
  the result as `needs clarification`, explain that it tested old code, and ask
  them to rerun from the PR worktree.

For command-based UAT, prefer the exact workflow or fixture named by the issue
and Review Agent evidence.

- Treat memory-tracker or fixture-only write-mode commands as operator-run
  smoke / functional verification evidence by default, not strict UAT. They are
  useful for confidence and can support acceptance, but they do not by
  themselves prove a real live workflow produced a real Project/PR result.
- Strict UAT should involve a human-selected live path or other operator-owned
  acceptance action that produces or confirms a real result. If the issue only
  provides high-safety fixtures, ask the operator whether those smoke results
  are sufficient for Human Review acceptance or whether to defer live UAT to the
  next lane.
- A controlled fixture workflow is valid UAT when the issue asks for a safe
  rehearsal path or fixture and the operator explicitly accepts fixture
  rehearsal as the UAT boundary. Otherwise record it as smoke evidence.
- The canonical workflow (`workflows/jade-symphony.md`) is a live lane command.
  Before asking the operator to run it in write mode, first ask for a dry-run
  and confirm the selected issue/PR is expected and safe.
- If the dry-run selects an unexpected live issue, stop and ask whether to
  defer, create a safer smoke target, or route to `Need Human Input`.
- If both fixture and live workflow checks are useful, run fixture checks first;
  treat live workflow dry-run/write as an explicit extra operator decision.

## Prepare Decision Note

Use `workflows/template/workpad/human-review.md` as the canonical note shape.
Complete it with the specific issue, PR, reviewer, decision, evidence reviewed,
UAT result, findings or missing evidence, and confirmation phrase.

Supported decisions:

- `Approve for Merging`: target state `Merging`.
- `Request Rework`: target state `Rework`.
- `Need Human Input`: target state `Need Human Input`.
- `Defer`: target state unchanged, unless the operator explicitly asks for a
  workpad-only defer note.

The note must distinguish Review Agent-owned evidence from Human Review-owned
UAT and acceptance. A Review Agent pass is input to Human Review, not a
substitute for Human Review.

## Confirm Before Mutating

Ask the operator for explicit confirmation before writing the decision note or
changing state. Examples:

- `confirm approve to Merging`
- `confirm request Rework`
- `confirm Need Human Input`
- `defer, do not change state`

Do not infer confirmation from discussion, enthusiasm, or a partial UAT answer.

## Record Decision Evidence

After explicit confirmation, write the completed decision note as append-only
timeline evidence before any state change.

Current safe route: do not use `project workpad`; write the completed note with
the CLI append-only timeline command and explicitly include the operator's
exact confirmation phrase.

```bash
cargo run -- project timeline-comment workflows/jade-symphony.md '#<issue>' /path/to/human-review-note.md --write
```

## Route With CLI

After decision evidence is recorded, set state as the final mutation.

Current grouped-command examples:

```bash
cargo run -- project set-state workflows/jade-symphony.md '#<issue>' merging --write
cargo run -- project set-state workflows/jade-symphony.md '#<issue>' rework --write
cargo run -- project set-state workflows/jade-symphony.md '#<issue>' need_human_input --write
```

After the state mutation, only read back:

```bash
cargo run -- project issue workflows/jade-symphony.md '#<issue>' --json
cargo run -- project state workflows/jade-symphony.md
```

Do not continue reviewing, implementing, or merging after the state change.

## Decision Mapping

- Approve for merge-lane work -> `Merging`.
- Confirmed implementation change needed -> `Rework`.
- Missing human decision, credential, external context, or destructive approval
  -> `Need Human Input`.
- Evidence incomplete but no routing decision yet -> no state change, or a
  workpad-only defer note if the operator explicitly wants it.

## Quality Bar

A good Human Review response leaves the operator with:

- the issue and PR identity;
- a plain-language explanation of what the issue/PR is about before UAT starts;
- a short evidence summary;
- a clear UAT result or UAT blocker;
- the Review Agent evidence boundary;
- a recommendation and supported alternatives;
- the exact state transition that will happen only after confirmation.
