# Runtime Artifact Storage Policy

Jade Symphony keeps GitHub Project and issue workpads as the shared source of
truth. Local files are execution artifacts: useful for resume, debugging, and
operator inspection, but not a substitute for tracker evidence.

## Artifact Classes

| Class | Durable? | Location |
| --- | --- | --- |
| Per-issue worktree | Recoverable until PR merge and tracker terminal state | `artifacts/<namespace>/<profile>/worktrees/` |
| Runtime state | Resume-critical while an issue is active | `artifacts/<namespace>/<profile>/runtime/` or configured `observability.logs_root/runtime/` |
| Event log | Durable local evidence | `artifacts/<namespace>/<profile>/logs/` |
| Review job artifact | Durable until review evidence is in the workpad | `artifacts/<namespace>/<profile>/reviews/` |
| PR body draft | Recoverable after PR creation | `artifacts/<namespace>/<profile>/drafts/pr-bodies/` |
| Workpad draft | Recoverable after tracker workpad upsert | `artifacts/<namespace>/<profile>/drafts/workpads/` |
| Reusable workflow/operator prompt | Durable repo material | `artifacts/<namespace>/<profile>/workflows/` until promoted to `docs/` or `examples/` |
| Disposable scratch file | Disposable | `artifacts/<namespace>/<profile>/scratch/` |

The default artifact root is `~/.jade-symphony/artifacts`. Workflows may set:

```yaml
artifacts:
  root: ~/.jade-symphony/artifacts
  namespace: Alive24/jade-symphony
```

If `namespace` is omitted, Jade Symphony derives one from `tracker.owner` and
`tracker.repo`, or from `tracker.project_slug`, or falls back to `local`.
Profiles add the final namespace segment so multiple worker identities do not
share worktrees, logs, or runtime state by accident.

## Promotion Rules

- Reusable workflow/operator instructions belong in `docs/` or `examples/`, not
  only in temp files.
- Tracker workpads remain the durable shared evidence surface for issue status,
  assumptions, decisions, verification, PR links, and review handoff.
- PR body drafts are temporary once the PR exists.
- Workpad drafts are temporary once the marker workpad has been upserted.
- Runtime state needed for resume must live under the configured logs/runtime
  root and should be referenced from the issue workpad when an active run pauses.
- Secrets must not be promoted into repo docs or logged artifacts.

## Cleanup Planning

`cleanup-plan` is dry-run only. It reports worktrees that are safe candidates for
operator removal when all of these are true:

- the tracker state is terminal;
- the linked PR is merged or closed;
- the local worktree branch matches the planned issue branch;
- `git status --porcelain` is clean;
- the worktree path still exists under the configured workspace root.

The command never deletes files:

```bash
cargo run -- cleanup-plan examples/github-project-workflow.md
```

Use the report to decide what to remove manually or in a future explicit
write-mode cleanup command. Temp cleanup is not proof that tracker evidence is
complete.
