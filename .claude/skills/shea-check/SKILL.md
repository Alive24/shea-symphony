---
name: shea-check
description: Refresh and assess current Shea Symphony execution posture without changing it. Use when an operator asks whether a named issue can proceed, what is running or blocked, what changed while they were away, what can run in parallel, or which Shea lane or repair Skill should handle the next action.
---

# Shea Check (Claude Code entry point)

This repository keeps one authoritative body for every operational Skill under
`.agents/skills/`, shared by all supported harnesses. This file only makes that
body discoverable from Claude Code; it adds no policy, permission, lane
authority or step of its own.

Read `.agents/skills/shea-check/SKILL.md` (relative to the repository root) now and
follow it verbatim, including every reference it resolves. Use
`docs/README.md` to route to the narrowest authoritative repository context.

If this file and the `.agents/` body ever disagree, the `.agents/` body wins;
report the divergence instead of reconciling it here.
