# Created Backlog

Dream run: `2026-05-23-01-post-app-server-dogfood-backlog`

All seeds were created through:

```bash
cargo run -- forge create --workflow workflows/shea-symphony.md --status backlog --assignee Alive24 --write
```

## Created Issues

- #380 `Backlog: make merge loop stack-aware for parent integration branches`
  - Theme: parent integration branch stack ordering.
  - Confidence: High.
- #381 `Backlog: align native subissue review evidence with Merging routing`
  - Theme: Review evidence text must match child routing to Merging.
  - Confidence: High.
- #382 `Backlog: quiet PR-link fallback comments for parent integration PRs`
  - Theme: PR-link repair comments after #337 and parent integration branch edge cases.
  - Confidence: Medium.
- #383 `Backlog: finalize terminal lane claim states after handoff and merge`
  - Theme: stale-active Main/Merge claim text after terminal or parked states.
  - Confidence: High.
- #384 `Backlog: prevent contradictory review usage-limit diagnostics`
  - Theme: successful Review output should not include stale failure diagnostics.
  - Confidence: High.
- #385 `Backlog: compact parent-batch Human Review UAT evidence`
  - Theme: parent-owned UAT evidence after child issues skip direct Human Review.
  - Confidence: Medium.
- #386 `Backlog: define scoped cross-repo Dream source inventory`
  - Theme: safe Issac/Shea conversation-log source window.
  - Confidence: High.
- #387 `Backlog: consolidate stale Issac tmux ADR into app-server runtime history`
  - Theme: historical ADR consolidation after app-server supersedes tmux default.
  - Confidence: Medium.
- #388 `Backlog: add post-merge app-server runtime smoke gate`
  - Theme: bounded app-server smoke before autopilot relies on #367.
  - Confidence: Medium.
- #389 `Backlog: design resilient Project write mutations`
  - Theme: Project write retry/fallback strategy after REST-first read work.
  - Confidence: High.

## Verification

- Forge output for each seed reported `forge_create=ok` and `project_status=Backlog`.
- `gh issue list --repo Alive24/shea-symphony --state open --limit 20 --json number,title,url,updatedAt` showed #380-#389 as the ten most recently updated open issues.
- `cargo run -- project state workflows/shea-symphony.md` after creation reported `Backlog:27`, up from the pre-run `Backlog:17`.
