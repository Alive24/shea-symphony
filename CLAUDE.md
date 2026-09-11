# Repository Instructions

This repository has no `AGENTS.md`. Start from [README.md](README.md) for the
public entrypoints, then use [docs/README.md](docs/README.md) as the coding
agent context router: it defines the authority order and selects the narrowest
current context for implementation, review and documentation reconciliation.

The operational Skills live under `.agents/skills/` and are shared by every
supported harness. `.claude/skills/` holds thin Claude Code entry points that
delegate to those bodies; it is not a second source of truth, and it is not
part of the first-party Skill inventory that `tests/skill_suite.rs` guards.
