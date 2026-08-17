# Installable Resource Manifest

After resolving the stable release to one immutable commit, read
`.shea/resources.v1.json` from that commit before selecting or fetching target
resources. Require the supported schema version, one available non-optional
core group, explicit optional groups, declared dependency edges, and paths that
remain inside the release checkout.

Install the complete core closure by default. `setup-shea` is global and must
not appear in any target-vendored group. Optional groups require explicit
operator selection; include their transitive dependencies exactly once. Reject
unknown, unavailable, cyclic, missing, empty, escaping, or duplicate resource
entries before presenting a write plan. An unavailable declared extension such
as a future `shea_docs` group is not silently substituted.

Expand Skill and prompt/template directories to the exact staged files for
digesting, conflict classification, and readback. Preserve each manifest kind
(`skill`, `workflow`, `contract`, `adapter`, `lane_prompt`, `backend_prompt`,
`template`, `report`, or `documentation`) in the plan so readiness can report
the selected groups and exact Markdown sources.

The manifest is a source declaration, not a target lockfile or package manager.
Do not copy it as setup ownership metadata, infer deletion authority from it,
or use it to overwrite target-customized bytes.
