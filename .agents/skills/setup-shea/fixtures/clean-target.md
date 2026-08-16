# Clean Target Initial Setup

## Observed input

- Target repository has no `.agents/skills` or `.shea` contract files.
- Codex is detected; Claude Code and Antigravity are not detected.
- Latest stable release resolves to tag `v1.2.3` and full commit
  `1111111111111111111111111111111111111111`.

## Expected plan

- Classify selected project-local Skills and selected canonical Markdown as
  `add` from the one pinned commit.
- Present exact paths, bytes/digests, runtime-profile proposal, verification,
  and any external Project effects before confirmation.

## Expected result

- Apply only confirmed additions, verify readback, and report no lane claim or
  lane launch.
