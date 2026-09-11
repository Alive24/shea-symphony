---
name: setup-shea
description: Onboard or reconcile Shea Symphony in a target repository from one immutable stable GitHub release. Use for initial project-local setup, incomplete setup, environment drift, operator-requested reconciliation, or runtime-profile and repository-contract problems routed back from Doctor.
---

# Setup Shea (Claude Code entry point)

This repository keeps one authoritative body for every operational Skill under
`.agents/skills/`, shared by all supported harnesses. This file only makes that
body discoverable from Claude Code; it adds no policy, permission, lane
authority or step of its own.

Read `.agents/skills/setup-shea/SKILL.md` (relative to the repository root) now and
follow it verbatim, including every reference it resolves. Use
`docs/README.md` to route to the narrowest authoritative repository context.

If this file and the `.agents/` body ever disagree, the `.agents/` body wins;
report the divergence instead of reconciling it here.
