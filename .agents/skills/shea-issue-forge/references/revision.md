# Guarded Todo Revision

Use `issue.revise` only for an OPEN Todo issue with no active Main, Review, or
Merge claim. A native blocker does not make revision ineligible. Prepare the
complete replacement title/body through the workflow-selected executable-Issue
template and gate; do not use revision to edit state, assignees, relationships,
linked pull requests, or unrelated Project fields.

First run the adapter's read-only revision preview. Present its exact source,
target, active-workflow, and selected-template fingerprints and obtain explicit
confirmation of the emitted token. Before writing, re-read the issue, claims,
relationships, and PR evidence. Record prepared revision evidence before the
exact content edit, re-read once more, and stop on drift.

Read back exact title/body and every preserved tracker fact after the edit. An
uncertain edit is complete only when targeted readback proves the target and
preservation contract; source readback is `not_applied`, and any third state is
ambiguous. Keep the issue in Todo. An identical completed revision is
`already_applied` and performs no write.
