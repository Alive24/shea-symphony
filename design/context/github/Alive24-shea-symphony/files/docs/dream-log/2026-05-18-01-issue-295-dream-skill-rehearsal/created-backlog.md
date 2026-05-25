# Created Backlog

## #297 Backlog: harden lane claim worker token parsing

- Evidence anchor: #295 `session start` rejected the structured claim written
  with worker label `Codex Manual Main`.
- Coverage checked: #281 and CLI claim pointer docs.
- Promotion guidance: decide quote/escape/normalize/validate behavior before
  mutation.
- Dream confidence: High.

## #298 Backlog: tolerate session registry status drift in workspace reads

- Evidence anchor: `workspace show #295` failed on persisted status variant
  `recorded`.
- Coverage checked: session status documentation and Doctor integration gap.
- Promotion guidance: decide tolerance, migration, or repair diagnostic policy.
- Dream confidence: High.

## #299 Backlog: add CLI repair path for malformed Main Agent claims

- Evidence anchor: second `main claim` refused to supersede the malformed active
  claim, requiring break-glass Project field repair.
- Coverage checked: `review-clear-claim` and Doctor repair surfaces.
- Promotion guidance: define explicit operator confirmation and audit evidence
  for malformed active claim repair.
- Dream confidence: Medium.
