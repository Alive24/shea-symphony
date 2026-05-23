# Gemini Review

Status: unavailable.

## Command Tried

```bash
gemini --model gemini-3.1-pro-preview --approval-mode plan -p 'Review this Jade Symphony Dream run for duplicate risk, evidence quality, scope breadth, Backlog safety, and lane-authority risk. Do not mutate files or tracker state. Read docs/dream-log/2026-05-23-01-post-app-server-dogfood-backlog/RUN.md, created-backlog.md, topic-post-app-server-dogfood.md, topic-cross-repo-dream-sources.md, and docs/dream-log/INDEX.md. Return a concise PASS/CONCERNS result with bullets.'
```

## Failure Reason

The local approval reviewer rejected the command because it would send private repository Dream-log contents and backlog planning details to an external Gemini service. The rejection explicitly said not to work around the policy.

## Manual Review Notes

- Duplicate risk was checked against current open issues #305-#327, #329/#330, #344, #359/#362/#363, #364, and #367.
- The created seeds stayed non-dispatchable Backlog issues.
- The run log and topic logs include lane-authority notes that Dream output is advisory until promoted into an issue contract, docs, skill instructions, or CLI invariant.
- Two possible candidates were intentionally not created because current evidence was weak or covered: review job ledger normalization and a separate merge-loop dry-run stack warning.

## Later Reviewer Should Check

- Whether #385 overlaps too much with #316 before promotion.
- Whether #382 remains reproducible after #367/#364 context changes.
- Whether #388 should remain standalone or fold into #359 readiness after #367 Human Review.
