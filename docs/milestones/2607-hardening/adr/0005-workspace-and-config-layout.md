# ADR 0005: Workspace And Config Layout

Status: Proposed

## Context

The MVP can use vendored runtime bits in target repositories. That is convenient
for dogfood but blurs distribution, shared config, local runtime state, and
generated worktree ownership.

## Decision

Resolve the Symphony binary from the local install location. Do not vendor the
binary into target repositories.

During the App migration, package the versioned `shea-symphony-legacy` binary
as an App sidecar. The App atomically publishes a machine-local discovery
record only after validating the sidecar role, build identity, and digest.
Workspace `cli_path` remains the highest-precedence explicit override;
validated installed discovery is next; a cargo runner is debug-only.

Use the target repository canonical worktree as the source checkout. Store
tracked team shared config under repo `.shea/`, with `.shea/workflow.md` as the
preferred workflow file.

Store local runtime config, logs, state, and generated issue worktrees under
`~/.shea/` by default.

Config precedence is:

1. workspace-local config;
2. repo `.shea/` team shared config;
3. global `~/.shea/` config.

## Consequences

- Target repos can share workflow config without vendoring executables.
- Generated worktrees stay out of canonical worktrees.
- Local overrides remain possible.
- Automatic resolution rejects the Temporal worker and stale or tampered
  sidecars.

## Follow-Up

- Define `~/.shea/` path layout.
- Define migration behavior for repos with root `WORKFLOW.md`.
- Remove the Legacy discovery and bundle path when the App no longer depends on
  legacy product commands.
