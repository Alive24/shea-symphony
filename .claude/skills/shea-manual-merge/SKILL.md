---
name: shea-manual-merge
description: Execute one supervised Shea Symphony Merging-lane issue, including guarded landing, safe stale-base or conflict repair on the existing reviewed PR branch, evidence, and final readback.
---

# Shea Manual Merge (Claude Code entry point)

This repository keeps one authoritative body for every operational Skill under
`.agents/skills/`, shared by all supported harnesses. This file only makes that
body discoverable from Claude Code; it adds no policy, permission, lane
authority or step of its own.

Read `.agents/skills/shea-manual-merge/SKILL.md` (relative to the repository root) now and
follow it verbatim, including every reference it resolves. Use
`docs/README.md` to route to the narrowest authoritative repository context.

If this file and the `.agents/` body ever disagree, the `.agents/` body wins;
report the divergence instead of reconciling it here.
