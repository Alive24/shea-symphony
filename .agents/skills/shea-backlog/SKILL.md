---
name: shea-backlog
description: Run bounded Shea Symphony progress and blocker checkpoints, capture and organize residual Backlog memory, deduplicate or review stale seeds, and route operator-selected candidates to Issue Forge without promoting or executing them. Use when an operator asks what is moving, blocked, left over, worth remembering, or ready for later shaping.
---

# Shea Symphony Backlog

Maintain a compact memory surface for work that is not yet executable. Keep the
checkpoint useful now without turning Backlog into an execution queue.

## Bind and read

Resolve `.shea/contracts/workflow-capability.v1.md`, its active workflow, and a
supported adapter. Bind the repository, Project, default assignee, and operator-
named scope. Prefer current targeted issue, relationship, PR, workpad, and lane
evidence; inspect only the recent context needed to explain progress or preserve
residual work. Do not reopen broad history or archived Dream Logs by default.

Classify the request as one or more of:

- checkpoint current progress, blockers, and what can proceed;
- capture a bounded residual idea or repeated operator friction;
- organize or deduplicate existing Backlog seeds; or
- review stale seeds against current code, issues, and PRs.

Route only a concrete runtime or contract failure to
`$shea-doctor`; keep the overall checkpoint here.

## Report and capture

Report facts separately from inference. Summarize current movement, blockers,
next eligible work, and residual candidates with evidence anchors. Drop solved
or duplicate candidates and label uncertain ones instead of expanding the scan.

Use this compact seed when durable memory is warranted:

```markdown
## Issue Goal
<one concrete sentence>

## Why Remember This
<current evidence and residual value>

## Current Seed Scope
- <bounded thought, not an implementation contract>

## Open Questions for Issue Forge
- <what executable shaping must decide>
```

Keep the seed in `Backlog` and explain why it is not dispatchable. Create it
only after explicit operator confirmation of the exact title/body through the
guarded `issue.create` capability, then perform targeted readback. Do not modify
source, documentation, lane claims, or execution state.

## Stop boundary

Backlog never drafts or executes promotion, never claims Main, and never turns a
seed into Todo. When the operator selects a candidate for executable shaping,
stop and recommend `$shea-issue-forge`. Issue Forge owns the full
contract, quality gate, and any separately confirmed promotion.
