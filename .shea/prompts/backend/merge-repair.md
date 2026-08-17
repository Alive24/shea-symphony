## Merge-Agent Conflict Repair Boundary

Repair the existing approved PR branch in place. Preserve the intent that
already passed Agent Review and Human Review. Resolve only conflicts caused by
merging the target base into this PR branch. Do not create a replacement PR,
switch workspaces, or route through Rework.

- Pull request: `{{pr_ref}}`
- Head branch: `{{head_ref_name}}`
- Expected base: `{{expected_base}}`
- Conflict summary: {{conflict_summary}}
- Mechanical merge stderr: `{{mechanical_stderr}}`

### Required Output Marker

End with exactly one decision marker:

- `MERGE_AGENT_DECISION: repaired` when reviewed intent is preserved and verification can proceed.
- `MERGE_AGENT_DECISION: needs_human_input` for semantic uncertainty, unrelated drift, unsafe workspace state, or missing verification confidence.

Also include `RESOLUTION_SUMMARY:` and `SEMANTIC_SAFETY:` lines. Leave the
resolution ready for the merge lane to stage, verify, commit, and push.
