# Repository Contract Repair Reference

Use these structures for the Doctor skill's `repository_contract_repair` path.
Keep observed evidence separate from inference and keep confirmation scoped to
the exact displayed paths and diff.

## Contract Repair Plan

```markdown
## Shea Symphony Contract Repair Plan

- Target repository: `<owner/repo and root>`
- Workflow: `<resolved path>`
- Affected lane/model/harness: `<known value>` | `unknown`
- Result: `proposal` | `no_change` | `refused_unsafe` | `blocked`

### Observed evidence

- `<path or run evidence>`: `<direct observation>`

### Doctor inference

- Classification: `<classification>`
- Inference: `<reasoned conclusion>`
- Confidence: `high` | `medium` | `low`
- Alternatives or unknowns: `<facts still not proven>`

### Proposed bounded repair

- Allowed paths:
  - `<repository-owned path>`
- Remove: `<text/section>` | `none`
- Merge or consolidate: `<text/section>` | `none`
- Relocate: `<from -> to>` | `none`
- Add: `<missing execution-critical boundary>` | `none`
- Proposed unified diff: `<focused diff shown before confirmation>`

### Preserved invariants

- `<authority, claim, verification, PR, review, or state boundary>`

### Expected improvement

- `<behavioral expectation; do not use size alone as the pass criterion>`

### Verification

- `<workflow parse, render, metadata, fixture, or repository check>`
- Changed-path subset check: `<method>`
- Unrelated-byte preservation check: `<method>`

### Rollback

- `<restore the approved paths from pre-repair bytes or VCS>`

### Confirmation required

Confirm only the listed paths and displayed diff. Any material path or diff
change requires new confirmation.
```

## Doctor Contract Repair Evidence

Append this evidence through the configured issue timeline surface. Do not use
the Main Agent Workpad and do not mutate Project status.

```markdown
## Shea Symphony Doctor Contract Repair

- Run ID: `<doctor-contract-repair-id>`
- Target repository/workflow: `<repository>` / `<workflow path>`
- Result: `repaired` | `no_change` | `refused_unsafe` | `blocked`
- Confirmed paths: `<paths>` | `none`
- Confirmation evidence: `<operator confirmation>` | `not required: no write`
- Observed failure: `<facts>`
- Doctor inference and confidence: `<inference>` / `<confidence>`
- Classifications: `<classifications>`
- Affected lane/model/harness: `<known value>` | `unknown`
- Before/after summary: `<what was removed, merged, relocated, or added>`
- Preserved invariants: `<invariants>`
- Validation:
  - `<command/check>`: `<result>`
- Changed-path subset: `pass` | `fail`
- Unrelated target customizations: `byte-for-byte unchanged` | `<exception>`
- Tracker state: `unchanged`
- Rollback: `<restore instructions>`
- Remaining uncertainty or follow-up: `<risk>` | `none`
```

## Decision Rules

1. Prefer `no_change` when the contract already expresses the required behavior
   and no observed run evidence contradicts it.
2. Prefer subtraction or consolidation when it preserves the same effective
   invariant for every known consumer.
3. Use one concise addition only when the observed contract lacks an
   execution-critical boundary.
4. Refuse a simplification that removes the sole effective authority, safety,
   claim, verification, PR, review, or state-transition rule.
5. Treat model or harness behavior as affected only when evidence identifies
   it. Do not turn one run into a universal model preference.
6. Stop if approved-path bytes change between preview and application.
7. After application, fail closed if any changed path falls outside the
   confirmed set or any unrelated target customization changes.
