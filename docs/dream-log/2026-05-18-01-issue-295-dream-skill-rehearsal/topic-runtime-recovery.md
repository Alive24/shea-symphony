# Dream Topic: Runtime Recovery Friction

## Theme

Manual lane recovery still has a few sharp edges where tracker-visible claim
fields, local session registry evidence, and workspace discovery do not degrade
smoothly.

## Evidence Anchors

- `session start` rejected the first #295 claim with `invalid claim token Manual`
  after `main claim --worker "Codex Manual Main"` wrote a structured claim whose
  worker value contained spaces.
- A second `main claim --worker CodexManualMain` refused to supersede the
  malformed active claim, forcing break-glass `gh project item-edit` repair.
- `workspace show workflows/shea-symphony.md '#295'` failed with
  `session registry serialization error: unknown variant recorded`.
- `doctor workflows/shea-symphony.md` surfaced the same registry status drift as
  an integration gap while still producing other tracker findings.

## Existing Coverage Checked

- #281 covers manual lane session registry evidence, but not worker-token parser
  tolerance or malformed Main Agent claim repair.
- The CLI reference documents compact `v=1` lane claim pointers and manual claim
  commands.
- `review-clear-claim` exists for Review Agent cleanup, but no equivalent normal
  repair path was visible for malformed Main Agent claims.
- Session status docs list accepted status values, but existing persisted
  `recorded` values can still block `workspace show`.

## Candidate Triage

### Lane Claim Worker Token Parsing

- Backlog seed: #297
- Dream confidence: High
- Promotion path: Issue Forge should choose whether to escape, normalize, or
  pre-validate worker labels before mutation.

### Session Registry Status Drift

- Backlog seed: #298
- Dream confidence: High
- Promotion path: Issue Forge should choose whether unknown status values map to
  `unknown`, become repair diagnostics, or trigger migration.

### Malformed Main Agent Claim Repair

- Backlog seed: #299
- Dream confidence: Medium
- Promotion path: Issue Forge should define explicit operator confirmation and
  audit requirements for clearing or superseding malformed active claims.

## Coverage Decision

The candidates are related but not duplicates: #297 is parser/write validation,
#298 is persisted registry tolerance, and #299 is operator repair UX after a bad
claim already exists.

## Lane-Authority Note

This topic log is advisory only. Main, Review, Merge, and Doctor should not
treat it as an invariant unless a candidate is later promoted into an issue
contract or code/docs change.
