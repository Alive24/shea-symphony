---
name: shea-symphony-manual-merge
description: Execute one supervised Shea Symphony Merging-lane issue, including guarded landing, safe stale-base or conflict repair on the existing reviewed PR branch, evidence, and final readback.
metadata:
  short-description: Run one supervised merge lane
---

# Shea Symphony Manual Merge

Merge owns approved landing and narrowly safe merge-lane repair. It does not own Todo/Main implementation, independent Review, Human approval, or contract rewriting.

## Resolve and preflight

Read `.shea/contracts/workflow-capability.v1.md`, resolve the active workflow, and select its supported adapter. Use targeted `issue.read`, `issue.inspect`, `relationships.read`, `evidence.read`, and `pull_request.read`; the workflow owns target branches, merge policy, checks, workspace root, and backend.

Require Merging (or an explicitly recoverable merge-lane state), an empty/matching Merging claim, recorded Human approval, one reliable ready PR, correct base/head topology, current checks/review/mergeability, and one clean canonical PR worktree when local repair is needed. Preserve Main, Review, and Human evidence.

## Execute

Claim through `lane.claim` and read back. Clean approved landing should use the deterministic Merge surface. For BEHIND or content-conflicted PRs, repair only the reviewed intent against the current base on the existing PR branch, verify, push, append a standalone `Shea Symphony Merge Run`, and remain in Merging for mergeability reread. Do not route native subissue merge repair to `Rework`.

Route semantic ambiguity, dirty/untrusted workspace, missing approval/PR, failing verification/checks, or unavailable repair authority to Need Human Input with one concrete question. Transient unknown mergeability may remain Merging for retry.

Record merge/repair evidence before any state change. Done or Need Human Input is the final mutation; afterward perform readback only. Never delete the audit worktree/branch during landing, bypass explicit write confirmation, set Human Review, or substitute self-review.

For normal operations, prefer the operator-controlled `autopilot plan` / `autopilot loop` foreground workflow; this skill remains the supervised one-issue path.
