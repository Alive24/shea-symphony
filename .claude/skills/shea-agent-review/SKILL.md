---
name: shea-agent-review
description: Trigger one independent Shea Symphony Agent Review for a named ready issue through the external Review backend selected by the active workflow, then read back its recorded decision and routing.
---

# Shea Agent Review (Claude Code entry point)

This repository keeps one authoritative body for every operational Skill under
`.agents/skills/`, shared by all supported harnesses. This file only makes that
body discoverable from Claude Code; it adds no policy, permission, lane
authority or step of its own.

Read `.agents/skills/shea-agent-review/SKILL.md` (relative to the repository root) now and
follow it verbatim, including every reference it resolves. Use
`docs/README.md` to route to the narrowest authoritative repository context.

If this file and the `.agents/` body ever disagree, the `.agents/` body wins;
report the divergence instead of reconciling it here.
