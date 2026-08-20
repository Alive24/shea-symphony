---
name: shea-manual-merge
description: Execute one supervised Shea Symphony Merging-lane issue, including guarded landing, safe stale-base or conflict repair on the existing reviewed PR branch, evidence, and final readback.
metadata:
  short-description: Run one supervised merge lane
---

# Shea Symphony Manual Merge

Merge owns approved landing and narrowly safe merge-lane repair. It does not own Todo/Main implementation, independent Review, Human approval, or contract rewriting.

## Resolve and preflight

Read `.shea/contracts/workflow-capability.v1.md`, resolve the active workflow, and select its supported adapter. Use targeted `issue.read`, `issue.inspect`, `relationships.read`, `evidence.read`, and `pull_request.read`; the workflow owns target branches, merge policy, checks, workspace root, and backend.

Require Merging (or an explicitly recoverable merge-lane state), an empty/matching Merging claim, recorded Human approval, one reliable ready PR, correct base/head topology, current checks/review/mergeability, and one clean canonical PR worktree when local repair is needed. Preserve Main, Review, and Human evidence.

## Invocation authorization

An operator invocation bound to exactly one operator-selected issue—either named in the invocation or uniquely established by the operator in the current task—is explicit confirmation for one supervised Merge run. It authorizes the wrapper to claim that issue, safely repair and push only the existing reviewed PR when required, land the exact ready PR with the workflow merge policy, append Merge evidence, mark the claim complete, transition the issue to Done, and close it.

An issue discovered only through queue scanning, dry-run, or preflight is not operator-selected; show the prepared effect and obtain confirmation before merging it. Otherwise, after preflight proves the exact issue, PR, head/base revisions, merge policy, and bounded actions, execute without asking for the same authorization again. Require new explicit authorization if any resolved identity or merge policy changes, repair would exceed the reviewed intent, or preflight finds semantic ambiguity, dirty/untrusted state, missing approval, or unavailable authority. This invocation does not authorize another issue, PR, implementation scope, self-review, or Human approval.

## Execute

Claim through `lane.claim` and read back. Clean approved landing should use the deterministic Merge surface. For BEHIND or content-conflicted PRs, repair only the reviewed intent against the current base on the existing PR branch, verify, push, append a standalone `Shea Symphony Merge Run`, and remain in Merging for mergeability reread. Do not route native subissue merge repair to `Rework`.

Route semantic ambiguity, dirty/untrusted workspace, missing approval/PR, failing verification/checks, or unavailable repair authority to Need Human Input with one concrete question. Transient unknown mergeability may remain Merging for retry.

Record merge/repair evidence before any state change. Done or Need Human Input is the final mutation; afterward perform readback only. Never delete the audit worktree/branch during landing, treat queue selection as authorization, set Human Review, or substitute self-review.

For normal operations, prefer the operator-controlled `autopilot plan` / `autopilot loop` foreground workflow; this skill remains the supervised one-issue path.
