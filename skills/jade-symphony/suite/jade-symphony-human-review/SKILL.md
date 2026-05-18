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
- Use Jade Symphony CLI for Project reads, workpad writes, and confirmed state
  routing. Do not bypass it with raw Project mutations.

## CLI Topology Transition

Issue #284 is cleaning up Jade Symphony CLI topology. Prefer the intended grouped
language in explanations:

- `project state`
- `project issue`
- `project workpad`
- `project set-state`

During live use, if the current binary still exposes flat commands, use those
commands and say so in the workpad note:

```bash
cargo run -- project-state workflows/jade-symphony.md
cargo run -- project-issue workflows/jade-symphony.md '#<issue>' --json
cargo run -- workpad workflows/jade-symphony.md '#<issue>' /path/to/human-review-note.md --write
cargo run -- set-state workflows/jade-symphony.md '#<issue>' merging --write
```

Do not turn the topology transition into custom GitHub Project mutations.

## Required Reads

Before briefing the operator, read the decision surfaces:

```bash
cargo run -- project-issue workflows/jade-symphony.md '#<issue>' --json
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

## Guide UAT

Walk the operator through UAT items from the issue body.

- Treat unchecked UAT items as human-owned.
- Ask for concrete pass/fail/deferred notes when useful.
- If UAT cannot be performed, record what is missing.
- If UAT fails, recommend `Rework` with the smallest actionable finding.
- Do not check UAT boxes based only on Review Agent evidence.

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

## Route With CLI

After explicit confirmation, write the completed decision note through the Jade
Symphony workpad surface first. Then, if the decision requires a state change,
set state as the final mutation.

Current flat-command examples:

```bash
cargo run -- workpad workflows/jade-symphony.md '#<issue>' /path/to/human-review-note.md --write
cargo run -- set-state workflows/jade-symphony.md '#<issue>' merging --write
cargo run -- set-state workflows/jade-symphony.md '#<issue>' rework --write
cargo run -- set-state workflows/jade-symphony.md '#<issue>' need_human_input --write
```

After the state mutation, only read back:

```bash
cargo run -- project-issue workflows/jade-symphony.md '#<issue>' --json
cargo run -- project-state workflows/jade-symphony.md
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
