---
name: shea-symphony-human-review
description: Brief a Shea Symphony operator after independent review evidence, guide operator-owned UAT and explicitly authorized narrow remediation, record append-only evidence, and route only after explicit confirmation. Use for issues waiting in Human Review, including parent-batch acceptance.
metadata:
  short-description: Brief and route Human Review
  suite-version: 2026.08.15
---

# Shea Symphony Human Review

Human Review is the operator-owned acceptance checkpoint, not implementation, Agent Review, or merge execution.
Accepted Human Review routes to `Merging`, never directly to `Done`.

## Mandatory visible brief

After current reads and freshness preflight, but before UAT or routing, visibly
explain the review in the operator's language without requiring a GitHub read:

- **Problem**: what user, operator, or system problem the issue addresses.
- **Delivered change**: what behavior changed and where, not a raw diff.
- **Resulting effect**: the observed before/after outcome; if only intended or
  not yet evidenced, label that uncertainty explicitly.
- **Evidence**: what Agent Review and current readbacks establish, plus risks or
  missing evidence.
- **Human decision needed**: the remaining UAT or acceptance choice and the
  available routes.

Also name the issue, PR, branch/base, and current state. Never omit a field; use
`unknown`, `not evidenced`, or `not applicable` when necessary. Internal
reasoning, tool output, links, freshness status, and test summaries do not
satisfy this visible briefing contract.

## Bind and inspect

Resolve the active repository, workflow, tracker project, canonical harness,
linked PR worktree, decision-note template, and supported actions from current
configuration. Never assume repo-specific paths or command topology.

Use the configured Shea workflow surface for Project reads, append-only notes,
and guarded routing. Use provider views read-only for ordinary issue and PR
content; never bypass the workflow with raw Project mutations.

Before briefing, inspect the goal, scope, completion criteria, UAT, Main Workpad, evidence, Agent Review pass,
linked PR/checks, Project state, and stale assumptions. Summarize decision-relevant facts, not raw JSON.

Native GitHub subissues are not routine Human Review surfaces. Without recorded
`Subissue Human Review Exception: <reason>` evidence, explain that a passing
child routes from Agent Review to `Merging` and the parent owns final UAT.

Only for a native parent issue, read
`.shea/template/workpad/parent-batch-human-review-brief.md`. The first Human
Review action is to prepare a compact parent-batch evidence brief from current
readbacks. It is read-only and advisory: Do not write tracker comments or mutate
state while preparing it. Child `Done`, child PR merge evidence, and parent
Agent Review PASS are inputs, not proof that parent UAT passed or authorization
for approval.

## Authority and language

- Do not change implementation except a narrow mechanical PR freshness repair
  or the explicitly authorized UAT remediation below.
- Do not act as Agent Review or Merging Agent, and do not merge.
- Never mutate Project state until the operator explicitly confirms the final
  decision after the briefing and UAT discussion.
- Treat unchecked UAT as human-owned unless the issue says otherwise.
- Keep Human Review notes append-only; never overwrite the Main Workpad.
- Match the operator-facing language. Do not force English in live discussion.
  Write durable tracker artifacts in English and preserve canonical decision
  labels, state names, paths, and commands.

## Review flow

1. Inspect the decision surfaces.
2. Run the PR freshness preflight automatically; it is not operator-owned UAT.
3. Present one current five-field brief and include a running note draft. If
   preflight is blocked, label that evidence gap and stop before UAT.
4. Give exactly one next UAT action, why it matters, where to run it, and ask for
   `pass`, `fail`, `deferred`, or the smallest blocker.
5. Wait for the operator result; do not infer acceptance from tests or tone.
6. Draft the English decision note and state the proposed route.
7. Obtain an explicit confirmation phrase before writing evidence or routing.
8. Append the note, perform the guarded state route as the final mutation, then
   only read back status and run Doctor verification.

## PR freshness preflight

Run from the linked PR/issue worktree, never canonical `main`:

1. Fetch upstream and verify the branch contains latest `origin/main`.
2. If behind, attempt only a safe mechanical merge of `origin/main`.
3. If clean or mechanically resolvable, run focused verification, push the
   branch, and record the repair in the running note draft.
4. If conflicts are broad, product-scope, ambiguous, or verification fails,
   stop before UAT and recommend `Request Rework` with the smallest finding.

If no safe linked worktree can be resolved, record a UAT blocker and ask for the
smallest workspace choice. Provider mergeability is corroborating evidence, not
a substitute for the local ancestry check.

## Guide UAT

- Guide one checklist item at a time unless the operator requests the full list.
- Run PR-specific checks from the linked PR worktree; a result from canonical
  `main` before merge needs clarification and rerun.
- Do not mark human-owned UAT from Agent Review or automated test evidence.
- Treat fixture or memory-tracker runs as smoke evidence unless the issue asks
  for rehearsal and the operator accepts that boundary.
- Before any configured live workflow write, show its dry-run target and obtain
  the operator's explicit choice to exercise that live path.
- Keep a running note draft separating Agent Review, automatic preflight, and
  operator-owned UAT evidence.

## Operator-authorized UAT remediation

When UAT reveals a concrete defect, repair it only when the operator explicitly
asks, it stays narrow and local to the linked PR, and the contract is unchanged:

1. State the defect, intended repair, and affected verification before editing.
2. Work only in the linked issue/PR worktree.
3. Stop and recommend `Request Rework` if scope becomes broad or ambiguous.
4. Run focused tests and required verification; present the diff, new revision,
   resulting effect, residual risk, and remaining UAT.
5. Mark the prior Agent Review PASS stale. Self-verification is not independent
   review, so do not resume acceptance on the changed revision.
6. Draft an append-only UAT Remediation note and obtain explicit confirmation
   before writing it and routing to `Agent Review` as the final mutation.
   A fresh independent Agent Review pass is required before Human Review resumes.

## Decision evidence and routing

Use `.shea/template/workpad/human-review.md`. Show the completed draft before
asking for an explicit phrase such as `confirm approve to Merging`. A UAT result
alone is not confirmation. After confirmation, append the decision note before
the state transition.

| Decision | Route |
| --- | --- |
| `Approve for Merging` | `Merging` |
| `Request Rework` | `Rework` with an actionable finding |
| `Need Human Input` | `Need Human Input` with the unresolved question |
| `Defer` | retain `Human Review`; append a note only if requested |

## Quality gate

Do not approve when the issue contract, linked PR, Agent Review evidence,
freshness result, operator-owned UAT, visible five-field brief, or explicit
confirmation is missing. Do not replace UAT with technical verification, hide
uncertainty, treat remediation self-verification as independent review,
overwrite evidence, or continue implementing or merging after the route.
