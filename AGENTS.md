# Repository Guide

Start from [README.md](README.md) for the public entrypoints. [docs/README.md](docs/README.md)
is the coding agent context router: it owns the authority order and selects the
narrowest current context for a task. Do not load `docs/` wholesale.

## Interaction Language

Reply to the user in the language the user writes to you in. Keep doing so for
the whole task, including while running a Shea lane. Switch only if the user
asks for another language.

Write everything that is persisted in English: repository content, and every
issue/PR title, body and comment, workpad, review, evidence record and
publication draft. Keep machine-recognized values in their original form —
configuration keys, Liquid variables, state enums, and recognized headings and
markers.

These two rules are independent: writing an English workpad does not make the
conversation English. This section is their single source, so the Skills and
lane prompts do not restate them.

## Skills

`.agents/skills/` holds the single authoritative body of every operational Skill
and is harness-neutral. Each harness contributes only discovery metadata:
`agents/openai.yaml` for Codex and a thin delegating `SKILL.md` under
`.claude/skills/` for Claude Code. Do not fork Skill policy per harness, and do
not treat a harness entry point as lane authorization. `.claude/skills/` is not
part of the first-party inventory that `tests/skill_suite.rs` guards.
