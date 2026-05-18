# Gemini Review

Command:

```bash
/opt/homebrew/bin/gemini --skip-trust --approval-mode plan -p "Review this Jade Symphony Dream rehearsal summary for duplicate risk, evidence quality, scope, and lane-authority safety. Summary: Issue #295 adds a separate jade-symphony-issue-forge-dream skill, docs/dream-log/INDEX.md, docs updates, and creates Backlog seeds #297 lane claim worker token parsing, #298 session registry status drift, #299 malformed Main Agent claim repair. Respond with concise pass/follow-up notes only."
```

Result:

- Duplicate risk: pass.
- Evidence quality: follow-up; the initial summary lacked concrete links or
  excerpts.
- Scope: pass.
- Lane-authority safety: pass.

Follow-up applied:

- `RUN.md` now lists the exact commands and created issue numbers.
- `topic-runtime-recovery.md` records the concrete failure text and coverage
  checks.
- `created-backlog.md` maps each created Backlog seed to its evidence and
  confidence.
