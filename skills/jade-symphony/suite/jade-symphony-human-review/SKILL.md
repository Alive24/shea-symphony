---
name: jade-symphony-human-review
description: Use when briefing a Jade Symphony operator for Human Review after independent Review Agent pass evidence, guiding UAT, recording a structured decision note, and routing only after explicit operator confirmation.
metadata:
  short-description: Jade Symphony Human Review briefing
  suite-version: 2026.05.18
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

- Do not modify implementation code.
- Do not act as the independent Review Agent.
- Do not merge PRs or act as the Merging Agent.
- Do not move accepted work directly to `Done`.
- Accepted Human Review routes to `Merging`.
- Treat UAT checklist items as Human Review-owned unless the issue explicitly
  says otherwise.
- Never mutate Project state until the operator explicitly confirms the decision
  after the briefing and UAT discussion.
- Use Jade Symphony CLI for Project reads and confirmed state routing. Do not
  bypass it with raw Project mutations.
- Human Review decision notes are append-only timeline evidence. They must not
  overwrite or restructure the canonical Main Agent Workpad.

## CLI Topology Transition

Issue #284 is cleaning up Jade Symphony CLI topology. Prefer the intended grouped
language in explanations:

- `project state`
- `project issue`
- `project set-state`

Do not use `project workpad` for Human Review decision notes. That command
upserts the canonical Main Agent Workpad marker comment and is reserved for
Main implementation evidence, including Main-lane Rework rounds.

During live use, if the current binary still exposes flat commands, use those
commands and say so in the decision note:

```bash
cargo run -- project state workflows/jade-symphony.md
cargo run -- project issue workflows/jade-symphony.md '#<issue>' --json
cargo run -- project set-state workflows/jade-symphony.md '#<issue>' merging --write
```

Do not turn the topology transition into custom GitHub Project mutations.
Until Jade Symphony CLI exposes a first-class append-only timeline comment
command, record this as a CLI surface gap in the decision note and use
`gh issue comment` only for the Human Review decision note. Project status
changes must still go through `project set-state`.

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

## Brief The Operator

Give a concise Human Review brief with:

- what the issue was supposed to deliver;
- what changed and where the PR is;
- what the Review Agent already checked;
- what remains human-owned, especially UAT;
- any missing evidence, stale assumption, or risk;
- available decisions and their target states.

If the issue is not in `Human Review`, or if Review Agent pass evidence or a
reliable linked PR is missing, stop before UAT and recommend the smallest safe
route such as `Need Human Input`, `Agent Review`, or no state change.

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

Preferred future route: use the first-class Jade Symphony CLI append-only
timeline/comment command once it exists.

Current safe route: do not use `project workpad`; instead write an issue comment
with the completed note and explicitly include:

- `CLI gap: no append-only Human Review timeline command is available yet`;
- `Project state mutation will be performed through Jade Symphony CLI`;
- the operator's exact confirmation phrase.

Current fallback example:

```bash
gh issue comment <issue> --repo Alive24/jade-symphony --body-file /path/to/human-review-note.md
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
- a short evidence summary;
- a clear UAT result or UAT blocker;
- the Review Agent evidence boundary;
- a recommendation and supported alternatives;
- the exact state transition that will happen only after confirmation.
