---
name: shea-human-review
description: Brief a Shea Symphony operator after independent Review, guide operator-owned UAT and narrow authorized remediation, record explicit decisions, and route only after confirmation.
metadata:
  short-description: Guide operator-owned Human Review
---

# Shea Symphony Human Review

Human Review is the operator-owned acceptance checkpoint. Accepted Human Review routes to `Merging`, never directly to Done.

## Mandatory visible brief

Before asking for a decision, show:

- **Problem**: the user/operator/system problem.
- **Delivered change**: what changed and where.
- **Resulting effect**: observed before/after behavior, or clearly labeled intent.
- **Evidence**: current issue, Main workpad, Review run, PR/checks, relationships, and risks.
- **Documentation Impact**: the Issue declaration, bounded Main evidence, and actual PR documentation diff.
- **Human decision needed**: remaining UAT or acceptance choice.

## Resolve and inspect

Read `.shea/contracts/workflow-capability.v1.md`, resolve the active workflow, and select a supported adapter. Use targeted issue/evidence/PR/relationship reads. Require Human Review state, independent PASS evidence for the current PR revision, one reliable ready PR/workspace identity, and no stale or conflicting claim.

Routine native children normally pass Agent Review to Merging; the parent owns Human Review/UAT unless an explicit exception says otherwise. When the `parent_subissues` resource group is enabled, parent-batch readiness reports use `.shea/template/report/parent-batch-readiness-report.md` and remain read-only.

## Guide UAT and decision

Keep Review evidence, automatic preflight, and human-observed UAT distinct. Do not mark human UAT complete from Main or Review claims. Use `.shea/template/decision/human-review.md` for the append-only decision draft.

Classify Documentation Impact reconciliation as exactly `complete` or `reconciliation required`. Approval for Merging is unavailable while reconciliation is required. Resolution may come from ordinary Main/Rework, a narrow manual repair, or optional `shea-docs`; never require that optional skill. Record the compared Issue declaration, bounded Main evidence, PR diff, and any remaining concern in the decision draft. Merging does not own this decision.

Supported decisions are Approve for Merging, Request Rework, Need Human Input, and Defer. Show the complete draft and exact intended route first. Never mutate Project state until the operator explicitly confirms the bound decision. Append decision evidence before applying `issue.transition`; state is the final mutation, followed by targeted readback only.

## Authorized UAT remediation

With explicit narrow authority, repair only the discovered acceptance defect on the existing branch/worktree, verify, push, update the canonical Main workpad, and mark the prior Review PASS stale. Record remediation evidence before routing back to Agent Review as the final mutation. A fresh independent Review PASS is required before Human Review resumes.

Never self-approve, merge, overwrite the Main workpad, infer confirmation, or describe unobserved behavior as verified.
