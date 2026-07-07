# App And CLI Split

Status: Draft

## Principle

The CLI/Symphony runtime is the execution authority. The App is a controlled
operator surface.

## App May

- display state-grouped workflow structure;
- display current workflow step and issue state;
- display snapshots;
- display logs, traces, and artifacts after issue-level drill-down;
- start, stop, or tick Autopilot through controlled Symphony commands;
- trigger display-oriented commands such as snapshot read and tracker cache
  refresh;
- show disabled or bypassed workflow steps when available.

## App Must Not

- directly mutate tracker state;
- directly edit worktrees;
- bypass Symphony transition checks;
- perform hidden write operations during refresh.

## First App Target

Read-only dashboard and workflow visualization:

- current operational lane items;
- human todo items;
- concise PR number/status;
- current issue state;
- state-grouped workflow steps;
- blocked/needs-input markers that already map to tracker/workflow state;
- latest evidence links, without eager artifact reads.

Manual graph editing belongs to 2608 Workflow Graph Extension or later.

Worktree path, branch name, full trace detail, and artifact bodies belong in
lane item detail, not top-level dashboard refresh.
