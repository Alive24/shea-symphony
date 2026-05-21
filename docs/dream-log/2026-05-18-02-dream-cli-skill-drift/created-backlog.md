# Created Backlog

## #319 Backlog: handle Codex config-migration prompts in tmux lanes

- URL: https://github.com/Alive24/jade-symphony/issues/319
- Reason: merge-lane tmux startup for #298 reached an external-agent config
  migration prompt after workspace trust, stopping prompt injection with
  misleading trust-prompt wording.
- Scope: classify or safely preflight Codex config/onboarding prompts in
  supervised tmux lane startup.

## #320 Backlog: validate repo-owned skill command examples

- URL: https://github.com/Alive24/jade-symphony/issues/320
- Reason: the repo-owned Dream skill contained stale command examples, and a
  live run of the top-level `inspect` example failed against the current
  grouped CLI topology.
- Scope: read-only validation or checklist for runnable command examples inside
  repo-owned Jade Symphony skills.
