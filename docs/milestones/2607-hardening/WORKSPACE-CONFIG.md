# Workspace And Config Layout

Status: Draft

## Problem

The MVP can use a vendored Symphony runtime in target repositories. That works
for dogfood but is not the desired distribution or workspace model.

## Desired Layout

Symphony binary:

- resolved from a validated local install record;
- not vendored into target repositories.

Canonical worktree:

- an already cloned project directory;
- contains tracked source and tracked shared config.

Repo shared config:

- `.shea/` in the canonical worktree;
- tracked by the target repository;
- contains team/shared workflow config such as `.shea/workflow.md`.

Local runtime root:

- defaults under `~/.shea/`;
- contains local config, runtime state, logs, and generated worktrees;
- not inside the canonical worktree by default.

## Config Precedence

Highest to lowest:

1. workspace-local config;
2. repo `.shea/` team shared config;
3. global `~/.shea/` config.

## Transitional App CLI Resolution

While the App still needs the legacy operator command graph, it resolves its
CLI in this order:

1. workspace `cli_path`;
2. validated machine-local discovery at
   `~/.shea-symphony/runtime-discovery.json`;
3. debug-only `cargo run --bin shea-symphony-legacy` from the configured engine
   checkout.

The packaged App publishes the discovery record atomically from its bundled
target-specific sidecar. Installed discovery requires the `legacy_cli` role,
the `shea-legacy-cli-v1` contract, matching App build metadata, and a matching
SHA-256 digest. Only an explicit `cli_path` may temporarily select an unmarked
2606 executable; automatic discovery never does.

## Worktree Rule

Symphony-created issue worktrees should live under the local runtime root by
default, not inside the canonical repository worktree.

## Open Questions

- Exact local runtime directory naming by owner/repo/profile.
- Whether `.shea/workflow.md` replaces or augments root `WORKFLOW.md` in legacy
  repos.
