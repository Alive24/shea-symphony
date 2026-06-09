# Dream Topic: Post-App-Server Dogfood Backlog

## Theme

Recent dogfood moved Shea Symphony from tmux-first Main and merge-agent runtime toward app-server-first execution, while parent/subissue and Review/Merge lanes continued to mature. This topic captures second-order backlog candidates surfaced by that transition rather than duplicating the primary implementation issues.

## Evidence Anchors

- `#367`: parent app-server batch is in Human Review with PR #379 and all child subissues Done.
- `#369`: Main lane defaults to app-server, but Review evidence includes a contradictory usage-limit diagnostic before a PASS response.
- `#370` and `#371`: child merge evidence records manual merge completion because merge-loop dry-run selected a later stacked child first.
- `#369`, `#370`, and `#371`: Review evidence text says ready for Human Review / target `human_review`, while routine native child routing should go to Merging.
- `#369`, `#370`, and `#371`: terminal Project readbacks still show active Main or Merging Agent claim strings after Done.
- `project state`: still reports GitHub Project v2 PR linking and live write methods as integration gaps.
- `doctor`: no blockers, but body-only parent hierarchy warnings remain for #318, #359, and #364.

## Candidate Triage

### Merge Loop Stack-Aware Selection

- Backlog seed: #380.
- Dream confidence: High.
- Why kept: live merge evidence shows the operator manually preserved stack order because the generic loop selected #369 before #370/#371.
- Existing coverage checked: #274 covers branch targeting, #317 covers merge-loop repair, and #364 covers relationship commands; none own stack-order queue selection.

### Native Subissue Review Evidence Routing

- Backlog seed: #381.
- Dream confidence: High.
- Why kept: routing behavior and evidence wording diverged on #369/#370/#371.
- Existing coverage checked: #358 owns parent-owned child routing semantics, but not generated Review comment wording.

### Parent Integration PR-Link Fallback Comments

- Backlog seed: #382.
- Dream confidence: Medium.
- Why kept: #337 quieted the normal PR-link path, but parent integration branch PRs still needed link repair and may still produce noisy comments.
- Existing coverage checked: #337 covers workpad/noisy comment cleanup in normal paths; #364 may later provide better relationship support.

### Terminal Claim Finalization

- Backlog seed: #383.
- Dream confidence: High.
- Why kept: Project readbacks show `state=active` lane claims on Done or Human Review items.
- Existing coverage checked: #307, #299, and #338 cover adjacent claim/registry problems, but not a unified terminal claim finalization contract.

### Review Usage-Limit Diagnostic Contradiction

- Backlog seed: #384.
- Dream confidence: High.
- Why kept: #369 renders a failure-looking diagnostic inside an otherwise successful Review evidence comment.
- Existing coverage checked: #312 covers retry/backoff health; #313 covers live status; neither owns final evidence consistency.

### Parent-Batch Human Review UAT Evidence

- Backlog seed: #385.
- Dream confidence: Medium.
- Why kept: #367 now has a parent-owned UAT checklist after multiple child PRs and reviews; operator friction is evidence aggregation, not another child implementation.
- Existing coverage checked: #316 is related batch handoff work; promotion should decide whether #385 is distinct or folds into #316.

### Post-Merge App-Server Smoke Gate

- Backlog seed: #388.
- Dream confidence: Medium.
- Why kept: #367 proves implementation and Review, but #359 Autoloop should not rely on app-server without a narrow post-merge smoke gate.
- Existing coverage checked: #367 owns parent UAT, #359 owns autopilot, and #318 owns progress heartbeat behavior.

### Resilient Project Write Mutations

- Backlog seed: #389.
- Dream confidence: High.
- Why kept: #347 moved reads REST-first, but Project writes still depend on `gh api graphql`; recent dogfood repeatedly paid time for transient GitHub write/readback behavior.
- Existing coverage checked: #303 centralizes access, #347 covers reads, and #364 covers relationship operations; write mutation resilience is broader.

## Watchlist / Not Created

- Review job ledger normalization was considered, but current `_369-gemini-1779472192752-1.json` already includes `issue_ref`, `decision_outcome`, and `decision_target_state`, so the earlier concern looked stale.
- Merge-loop dry-run stack warning was not split from #380 because it belongs naturally inside stack-aware selection.
- Doctor topology warning repair UX was not split from #364/#389; relationship commands and write mutation resilience should land first.

## Lane-Authority Note

This topic log is advisory only. Main, Review, Merge, Human Review, and Doctor should not treat these observations as workflow invariants until the corresponding Backlog seed is promoted into an issue contract or a repo-owned invariant.
