# Dream Topic: Cross-Repo Dream Source Boundaries

## Theme

The operator asked this Dream run to consider conversation records opened from both `issac` and `shea-symphony`. That exposed a recurring source-window problem: Dream needs enough cross-repo context to preserve architectural memory, but broad raw log sweeps are unsafe and noisy.

## Evidence Anchors

- `issac/Journal/dream-index.md`: says ADRs and architectural memory remain largely undreamed.
- `issac/ADRs/26051500-shea-symphony-tmux-supervision/README.md`: records a tmux-first supervision plan that is now partly superseded by #367 app-server runtime work.
- `issac/ADRs/26050700-shea-harness-workflow/symphony-workflow-reference.md`: older workflow reference captures Human Review and workpad semantics from the Linear/Shea harness era.
- `MEMORY.md`: includes relevant rollout summaries from both `cwd=/Volumes/Bohemialive/GitHub/issac` and `cwd=/Volumes/Bohemialive/GitHub/shea-symphony`.
- `rollout_summaries/2026-05-19T05-51-22-eohU-sheaintel_elmore_usecase_and_dream_skill_governance.md`: records the Dream governance boundary that internal docs can change directly, but skills and external-facing surfaces stay proposal-only.

## Candidate Triage

### Scoped Cross-Repo Dream Source Inventory

- Backlog seed: #386.
- Dream confidence: High.
- Why kept: the current run needed a safe alternative to broad JSONL sweeps, and the operator explicitly asked for Issac and Shea contexts to both be considered.
- Existing coverage checked: the Dream skill mentions recent rollout summaries, but does not define a durable allowlisted cross-repo source protocol.

### Issac tmux ADR Consolidation

- Backlog seed: #387.
- Dream confidence: Medium.
- Why kept: the Issac tmux supervision ADR is historically valuable but stale after #367; future Dream or operator docs should preserve the transition rather than silently relying on the old substrate.
- Existing coverage checked: #367 implements the runtime shift and #321 covers app-server continuation parity, but neither owns historical ADR consolidation.

## Watchlist / Not Created

- Full Issac ADR Dream pass: kept out of this Shea-focused Dream run because it would broaden into Isaac architectural memory rather than immediate Shea Symphony backlog.
- Dream as a first-class CLI namespace: kept out because the Dream governance rollout already noted it, but this run did not gather enough new evidence to create a non-duplicate seed beyond #386.
- Editing skills directly: explicitly not done because Dream governance says skills should remain proposal/Backlog output during Dream.

## Lane-Authority Note

Cross-repo Dream context is advisory. Issac ADRs and memory summaries can inform backlog shaping, but Shea Symphony lane agents should only follow issue bodies, repo-owned docs/skills, CLI invariants, and live Project state.
