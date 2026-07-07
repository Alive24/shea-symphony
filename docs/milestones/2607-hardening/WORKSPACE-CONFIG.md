# Workspace And Config Layout

Status: Draft

## Problem

The MVP can use a vendored Symphony runtime in target repositories. That works
for dogfood but is not the desired distribution or workspace model.

## Desired Layout

Symphony binary:

- resolved from local install location;
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

## Worktree Rule

Symphony-created issue worktrees should live under the local runtime root by
default, not inside the canonical repository worktree.

## Open Questions

- Exact install path lookup order for the Symphony binary.
- Exact local runtime directory naming by owner/repo/profile.
- Whether `.shea/workflow.md` replaces or augments root `WORKFLOW.md` in legacy
  repos.
