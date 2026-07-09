---
name: shea-symphony-human-review
description: Use when briefing a Shea Symphony operator for Human Review after independent Review Agent pass evidence, guiding UAT, recording a structured decision note, and routing only after explicit operator confirmation.
metadata:
  short-description: Shea Symphony Human Review briefing
  suite-version: 2026.05.24
---

# Shea Symphony Human Review

Use this skill when the operator wants help reviewing a Shea Symphony issue that
has passed independent Review Agent checks and is waiting for Human Review.

Human Review is the operator-owned final acceptance checkpoint before merge-lane
work. It is not implementation work, it is not the independent Review Agent, and
it is not merge execution.

## Runtime Topology

Live Shea Symphony review routing still runs through the protected 2606 MVP
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

If the Tauri App is needed, start it from the MVP runtime `app/` directory with
the local MVP profile:

```bash
cd /Users/chuntengxiao/.shea-symphony/mvp/shea-symphony-2606-mvp/app
SHEA_SYMPHONY_APP_PROFILE_PATH=/Users/chuntengxiao/.shea-symphony/mvp/app-profile-shea-symphony.json npm run tauri -- dev
```

The profile points at the Shea Symphony target checkout and the MVP CLI binary.
Do not assume `npm run tauri -- dev -- --workdir <path>` alone keeps the backend
on MVP code.

Canonical decision note template:

```bash
workflows/template/workpad/human-review.md
```

Canonical parent-batch briefing template:

```bash
workflows/template/workpad/parent-batch-human-review-brief.md
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
- Use Shea Symphony CLI for Project reads and confirmed state routing. Do not
  bypass it with raw Project mutations.
- Human Review decision notes are append-only timeline evidence. They must not
  overwrite or restructure the canonical Main Agent Workpad.

## Conversation Language

- Match the operator-facing language to the current session's user language or
  the language already being used in the immediate review context.
- Do not force English for Human Review briefings, UAT guidance, summaries,
  back-and-forth discussion, or confirmation prompts when the operator is using
  another language.
- If the operator invokes the skill or discusses the review in Chinese, the
  live briefing, preflight readback, UAT request, and final confirmation prompt
  must be in Chinese by default.
- Durable written artifacts must be in English, even when the live conversation
  uses another language. This includes Human Review decision notes, issue
  bodies, timeline comments, workpad evidence, PR comments, and issue comments.
- Preserve exact command names, state names, file paths, issue titles, and
  decision labels in their canonical English form.
- Keep canonical values exactly where the template, command surface, or issue
  evidence expects fixed values, such as `Approve for Merging`, `Request
  Rework`, `Need Human Input`, `Defer`, `Merging`, `Rework`, and `Human
  Review`.

## CLI Topology Transition

Issue #284 is cleaning up Shea Symphony CLI topology. Prefer the intended grouped
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
cargo run -- project state workflows/shea-symphony.md
cargo run -- project issue workflows/shea-symphony.md '#<issue>' --json
cargo run -- project timeline-comment workflows/shea-symphony.md '#<issue>' /path/to/human-review-note.md --write
cargo run -- project set-state workflows/shea-symphony.md '#<issue>' merging --write
```

Do not turn the topology transition into custom GitHub Project mutations.
Project status changes must still go through `project set-state`, after the
timeline comment has been written.

## Required Reads

Before briefing the operator, read the decision surfaces:

```bash
cargo run -- project issue workflows/shea-symphony.md '#<issue>' --json
gh issue view <issue> --repo Alive24/shea-symphony --comments
gh pr view <pr> --repo Alive24/shea-symphony --json number,title,state,url,isDraft,baseRefName,headRefName,mergeStateStatus,reviewDecision,statusCheckRollup
```

Inspect the issue body and workpad for:

- issue goal, scope, guardrails, and dependencies;
- Expected Outcome, Completion Criteria, Functional Verification, UAT, and
  Context Verification checkboxes;
- Review Agent pass evidence and any explicitly unchecked review items;
- linked PR identity, readiness, base branch, and check/review state;
- missing evidence, stale assumptions, or blockers that should prevent approval.

For native parent issues, also read the current parent/subissue evidence before
briefing:

- native child issue list and each child's current Project state;
- child PR identity, target branch, and merge evidence into the parent
  integration branch;
- parent integration branch and parent final PR base/head evidence;
- parent Review Agent PASS evidence and any explicitly missing review evidence;
- remaining parent UAT checklist items that still need Human Review-owned
  pass/fail/deferred notes.

Do not drown the operator in raw JSON. Summarize the decision-relevant facts and
include exact issue and PR references.

## PR Freshness Repair Gate

Before any PR-specific UAT, verify that the reviewed PR branch contains the
latest `origin/main`. Run this only from the linked PR/issue worktree, never
from the canonical `main` checkout.

This gate is a required Human Review preflight, not an operator-owned UAT
decision. After the orientation brief, run the freshness check automatically
from the PR worktree before asking the operator to run or accept PR-specific
UAT. Do not ask for operator permission merely to run `git fetch`, the
`merge-base` check, or the safe mechanical repair path described below.
Operator confirmation is still required before final decision evidence or
Project state mutation.

Shea Symphony write-mode lane commands may now fast-forward the canonical
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
canonical `main` checkout. First try to select an existing PR branch worktree.
If no safe worktree can be selected or created from existing issue/PR evidence,
record the missing worktree as a UAT blocker and ask the operator for the
smallest missing workspace choice.

If `gh pr view` reports a non-clean merge state, treat that as corroborating
freshness or mergeability risk. The local `merge-base` check is still required
before PR-specific UAT because GitHub mergeability can lag or be temporarily
unknown.

## Live Output Contract

Human Review is an operator decision packet, not a freshness report. Freshness
and verification preflight are required, but they must stay subordinate to the
issue/PR briefing and the operator-owned UAT decision.

Use this live sequence:

1. Read the decision surfaces.
2. Give a plain-language orientation brief in the operator-facing language.
3. Run the automatic PR Freshness Repair Gate.
4. If the branch was refreshed or repaired, rerun the relevant verification.
5. Before asking for `pass`, `fail`, or `deferred`, present a post-preflight
   review packet that includes the issue purpose, what changed, Review Agent
   evidence, preflight result, remaining human-owned UAT, and decision boundary.
6. Ask for exactly one operator-owned UAT result.
7. Wait for the operator's result before drafting or writing the Human Review
   decision note.

The post-preflight review packet is mandatory whenever preflight work happened
after the initial orientation brief. Do not end a turn with only a freshness or
verification summary plus a bare `pass`/`fail`/`deferred` prompt.

Keep the freshness result compact:

- say whether the branch was fresh, mechanically refreshed, or blocked;
- include verification command outcomes when they changed confidence;
- mention GitHub mergeability recomputation only as a risk note;
- do not let stale-branch repair become the main story unless it blocks UAT.

Always include a short running Human Review note draft in the conversation after
non-trivial preflight work. The draft must separate Review Agent evidence,
automatic preflight/repair evidence, and operator-owned UAT still awaiting
feedback. This is live conversation state only; do not write it to the tracker
until the operator gives the explicit confirmation phrase.

## Brief The Operator

Before any freshness repair, operator-owned UAT command, decision-note drafting,
or state mutation, start with a plain-language orientation brief. The operator
should be able to understand what they are reviewing without opening GitHub
first. After that brief, required PR freshness checks run automatically under
the PR Freshness Repair Gate; they are not confirmation prompts.

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

Recommended post-preflight shape:

```text
## Human Review Packet

Issue: #<issue> <title>
PR: #<pr> <title or URL>
State: Human Review

What this is about: <one-sentence issue purpose>
What changed: <2-4 bullets summarizing the behavior being accepted>
Review Agent already checked: <short evidence summary>
Preflight status: <fresh / mechanically refreshed / blocked> plus verification summary
Human-owned UAT now: <one concrete inspection or command result needed>
Decision boundary: <pass/fail/deferred records UAT only; explicit confirmation is still required before note/state mutation>

Note draft so far:
- Review evidence: <short>
- Preflight: <short>
- Awaiting operator UAT: <short>
```

For parent issues with native subissues, explicitly summarize the parent/child
shape: which child issues are Done, which child PRs landed, which parent PR is
being accepted, and what combined behavior the parent UAT is meant to validate.

## Parent-Batch Brief

For native parent issues, the first Human Review action is to prepare a compact
parent-batch evidence brief from current readbacks before any UAT command,
freshness repair, decision-note drafting, timeline comment, or Project state
mutation.

Use `workflows/template/workpad/parent-batch-human-review-brief.md` as the
reusable brief shape. Keep it separate from
`workflows/template/workpad/human-review.md`; the parent-batch brief is not the
final Human Review decision note and must not replace or weaken the explicit
operator-confirmation boundary.

The parent-batch brief is read-only and advisory. Do not write tracker comments,
decision notes, Project state, PR state, issue bodies, workpads, or child
evidence while preparing it.

Build the brief in this order:

1. remaining parent UAT;
2. parent PR and readiness;
3. child batch table;
4. Review Agent evidence;
5. risks, stale assumptions, or missing evidence.

The brief must distinguish evidence already checked by Main, Review, and Merge
lanes from Human Review-owned UAT and acceptance. Child `Done`, child PR merge
evidence, and parent Review Agent PASS are inputs to Human Review; they are not
proof that parent UAT passed and they do not authorize approval without the
operator's explicit decision.

For the #400 evidence shape, the brief should be able to show remaining parent
UAT, parent PR #421 readiness, child #399/#383/#384 status and child PR merge
evidence, parent Review PASS evidence, and any risks or missing evidence before
asking the operator to run or confirm parent UAT.

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
- For an unmerged PR, automatically complete the PR Freshness Repair Gate from
  the PR worktree before presenting the first operator-owned UAT action.
- Report the freshness result succinctly before UAT: fresh, mechanically
  refreshed, blocked by missing worktree, or blocked by non-mechanical
  conflict/verification failure.
- After any non-trivial freshness repair, re-present the post-preflight review
  packet before asking for `pass`, `fail`, or `deferred`.
- Give exactly one next action, explain why it is the next action, and tell the
  operator what feedback to provide after running it.
- Tell the operator which directory to run the action from. For PR UAT, this is
  normally the reviewed PR/issue worktree, not the canonical `main` checkout.
- Wait for the operator's result before moving to the next operator-owned UAT
  action.
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
Why: <one sentence tied to the issue purpose>
Please reply with: pass/fail/deferred plus the observation or key output lines.
Reminder: this records UAT only; final routing still needs an explicit phrase
like `confirm approve to Merging`.
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
- Before PR-specific UAT, apply the PR Freshness Repair Gate automatically. Do
  not stop only because the PR branch is stale; first try the safe
  refresh/small-repair path.
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
- The canonical workflow (`workflows/shea-symphony.md`) is a live lane command.
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
cargo run -- project timeline-comment workflows/shea-symphony.md '#<issue>' /path/to/human-review-note.md --write
```

## Route With CLI

After decision evidence is recorded, set state as the final mutation.

Current grouped-command examples:

```bash
cargo run -- project set-state workflows/shea-symphony.md '#<issue>' merging --write
cargo run -- project set-state workflows/shea-symphony.md '#<issue>' rework --write
cargo run -- project set-state workflows/shea-symphony.md '#<issue>' need_human_input --write
```

After the state mutation, only read back:

```bash
cargo run -- project issue workflows/shea-symphony.md '#<issue>' --json
cargo run -- project state workflows/shea-symphony.md
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
