---
name: shea-backlog
description: Capture and organize bounded Shea Symphony Backlog memory, deduplicate or review stale seeds, and route operator-selected candidates to Issue Forge without promoting or executing them. Use when an operator wants to remember future work, add a Backlog seed, or organize, prune, or reconsider existing Backlog items.
---

# Shea Symphony Backlog

Maintain a compact memory surface for work that is not yet executable. Do not
use Backlog as the current execution-status or operator-navigation surface.

## Bind and read

Resolve `.shea/contracts/workflow-capability.v1.md`, its active workflow, and a
supported adapter. Bind the repository, Project, default assignee, and operator-
named scope. Prefer targeted issue, relationship, PR, and current-code evidence
needed to decide whether a seed remains useful. Do not reopen broad history or
archived Dream Logs by default.

Classify the request as one or more of:

- capture a bounded residual idea or repeated operator friction;
- organize or deduplicate existing Backlog seeds; or
- review stale seeds against current code, issues, and PRs.

Route current progress, blocker, readiness, or next-action assessment to
`$shea-check`. Route a concrete runtime or contract failure to `$shea-doctor`.

## Report and capture

Report facts separately from inference. Summarize retained, solved, duplicate,
and uncertain candidates with evidence anchors instead of expanding the scan.

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
