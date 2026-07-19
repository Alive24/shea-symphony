## Shea Symphony Parent-Batch Human Review Brief

This brief is read-only and advisory. It prepares the operator for UAT on a native parent issue, but it is not a Human Review decision note, does not record approval, and must not write tracker comments, Project state, PR state, issue bodies, workpads, or child evidence.

- Parent issue: #<parent-issue> <title>
- Parent PR: #<parent-pr> <url>
- Parent integration branch: `<branch>`
- Parent final PR base/head: `<base>` <- `<head>`
- Current Project state: `Human Review`
- Brief prepared from current readbacks at: <YYYY-MM-DD HH:MM timezone>

### 1. Remaining Parent UAT

List the parent UAT checklist items that still need Human Review-owned pass/fail/deferred evidence. Do not mark parent UAT complete based only on child `Done`, Main lane evidence, Merge lane evidence, or Review Agent PASS.

| Parent UAT item | Current evidence | Human-owned next action |
| --- | --- | --- |
| <unchecked or unconfirmed parent UAT item> | <none / current readback> | <operator action needed> |

### 2. Parent PR And Readiness

Summarize the parent final PR being accepted for `main`, using current PR and worktree readbacks rather than stale workpad text alone.

- Parent PR identity:
- Base/head branch:
- Draft state:
- Mergeability or freshness:
- Status checks / review state:
- Ready enough for UAT: yes / no / blocked because <reason>

### 3. Child Batch Table

Summarize the native child set and lane evidence already produced by Main, Review, and Merge lanes. Child `Done` means the child slice landed in the parent integration branch; it does not prove parent acceptance.

| Child issue | Project state | Child PR | PR target / merge evidence | Lane evidence checked |
| --- | --- | --- | --- | --- |
| #<child> <title> | `Done` / other | #<pr> | <merged into parent branch / missing> | <Main / Review / Merge evidence> |

### 4. Review Agent Evidence

Summarize parent Review Agent evidence for the parent final PR. Keep this separate from Human Review-owned UAT and acceptance.

- Review Agent result:
- Evidence location:
- Checks covered:
- Explicitly unchecked or deferred review items:

### 5. Risks, Stale Assumptions, Or Missing Evidence

List only issues that should block or shape the Human Review decision.

- Missing child status, child PR merge evidence, or native relationship:
- Parent PR freshness, draft, checks, or mergeability risk:
- Parent UAT evidence still missing:
- Stale assumptions that need current readback:
- Recommended next Human Review step:

### #400 Spot-Check Shape

For a parent issue shaped like #400, the brief should be able to show remaining parent UAT, parent PR #421 readiness, child #399/#383/#384 status and child PR merge evidence, parent Review PASS evidence, and any preserved risks without requiring the operator to open every child issue before UAT starts.
