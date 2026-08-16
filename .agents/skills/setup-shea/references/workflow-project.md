# Workflow And GitHub Project Binding

Use this phase when setup needs to add or reconcile Shea workflow contracts,
the selected capability adapter, App-profile resolution, or a GitHub Project v2
binding.

## Build The Repository Contract

1. Select a canonical workflow at the pinned commit and parse its front matter
   before proposing target-specific edits.
2. Resolve the workflow's capability contract, selected adapter, lane prompts,
   and workpad templates from their declared relative paths. Add those exact
   canonical paths to the staged resource plan; do not reconstruct their prose
   or use embedded fallback copies.
3. Bind target-owned values: tracker kind, repository owner/name, Project owner
   type/number, Status field and state map, assignee policy, base branch,
   workspace root, backend/harness choices, verification, and optional runtime
   profile.
4. Keep ecosystem discovery out of Shea core and preserve target-specific
   workflow customizations as conflicts for operator judgment.
5. Propose App-profile changes only when the chosen workflow or CLI cannot be
   resolved. Do not broaden setup into the shared/local profile redesign.

## Validate The Project Read-Only

Check repository identity, Project visibility, Project item access, required
Status options, lane-claim fields, authenticated actor, assignee policy, issue
workpad marker, and the selected adapter's compatibility. Distinguish missing
configuration from missing permission or credentials.

List every external Project addition or edit separately from repository-file
changes. Do not create or rename Project fields/statuses, add issues, claim a
lane, or change issue state unless the operator explicitly confirms that exact
external effect and a supported deterministic surface exists. Fail closed when
only an ambiguous or broad raw mutation is available.

Run the repository's workflow/config validation surface after confirmed file
writes, then repeat targeted tracker/Project reads. Adapter command syntax
belongs to the pinned adapter; do not bake a second command runbook into the
target Skill.
