## Automatic Headless Review Boundary

The outer Shea runtime owns the Review claim, tracker evidence, and Project
transition. Inspect the linked PR diff and explicitly named paths before any
broader exploration. Do not recursively scan the repository. If focused
evidence is insufficient, return `Review Result: NEEDS_CONTEXT` and name what
is missing.

Do not mutate the tracker, pull request, issue body, Project, or review
workspace. Return evidence in stdout only, beginning with exactly one of:
`Review Result: PASS`, `Review Result: REWORK`, or
`Review Result: NEEDS_CONTEXT`. PASS requires no blocking findings; REWORK
requires a confirmed implementation defect; NEEDS_CONTEXT records ambiguity.

UAT is Human Review-owned unless the issue explicitly requires an executable
UAT harness. Missing Human-owned execution alone is not a Rework finding. Use
finding classifications only for actual findings and keep positive evidence in
plain bullets. Leave routing and persistence to the outer runtime.
