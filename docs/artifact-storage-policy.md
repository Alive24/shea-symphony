# Runtime Artifact Storage Policy

Shea Symphony keeps GitHub Project and issue workpads as the shared source of
truth. Local files are execution artifacts: useful for resume, debugging, and
operator inspection, but not a substitute for tracker evidence.

## Artifact Classes

| Class | Durable? | Location |
| --- | --- | --- |
| Per-issue worktree | Recoverable until PR merge and tracker terminal state | `artifacts/<namespace>/<profile>/worktrees/` |
| Runtime state | Resume-critical while an issue is active | `artifacts/<namespace>/<profile>/runtime/` or configured `observability.logs_root/runtime/` |
| Event log | Durable local evidence | `artifacts/<namespace>/<profile>/logs/` |
| Rendered agent prompt | Durable local evidence for backend runs | `artifacts/<namespace>/<profile>/logs/prompts/` or configured `observability.logs_root/prompts/` |
| Review job artifact | Durable until review evidence is in the workpad | `artifacts/<namespace>/<profile>/reviews/` |
| PR body draft | Recoverable after PR creation | `artifacts/<namespace>/<profile>/drafts/pr-bodies/` |
| Workpad draft | Recoverable after tracker workpad upsert | `artifacts/<namespace>/<profile>/drafts/workpads/` |
| Reusable workflow/operator prompt | Durable repo material | `artifacts/<namespace>/<profile>/workflows/` until promoted to `.shea/workflows/` or `docs/` |
| Disposable scratch file | Disposable | `artifacts/<namespace>/<profile>/scratch/` |
| Canonical checkout quarantine | Operator decision required | `artifacts/<namespace>/<profile>/scratch/canonical-checkout-quarantine/` |

The default artifact root is `~/.shea-symphony/artifacts`. Workflows may set:

```yaml
artifacts:
  root: $SHEA_SYMPHONY_ARTIFACT_ROOT
  namespace: Alive24/shea-symphony
```

When `SHEA_SYMPHONY_ARTIFACT_ROOT` is unset, Shea Symphony resolves that token
to the default artifact root. Path suffixes are supported, so workflow roots can
derive related locations from one operator override:

```yaml
workspace:
  root: $SHEA_SYMPHONY_ARTIFACT_ROOT/Alive24/shea-symphony/default/worktrees
observability:
  logs_root: $SHEA_SYMPHONY_ARTIFACT_ROOT/Alive24/shea-symphony/default/logs
```

If `namespace` is omitted, Shea Symphony derives one from `tracker.owner` and
`tracker.repo`, or from `tracker.project_slug`, or falls back to `local`.
Profiles add the final namespace segment so multiple worker identities do not
share worktrees, logs, or runtime state by accident.

The canonical checkout is not an artifact store. It is the trusted launch
checkout for CLI and operator commands, while implementation, review, and merge
edits belong in verified issue worktrees. Live write lanes inspect the canonical
checkout before mutating tracker state. Tracked dirty files block the lane.
Recognized untracked runtime/log/prompt/evidence/draft scratch files are moved
under `scratch/canonical-checkout-quarantine/` with a manifest. Unclassified
untracked files block the lane so the operator can choose the correct issue
worktree, artifact location, or `.gitignore` rule.

## Promotion Rules

- Reusable workflow/operator instructions belong in `.shea/workflows/` or `docs/`, not
  only in temp files.
- Tracker workpads remain the durable shared evidence surface for issue status,
  assumptions, decisions, verification, PR links, and review handoff.
- PR body drafts are temporary once the PR exists.
- Workpad drafts are temporary once the marker workpad has been upserted.
- Runtime state needed for resume must live under the configured logs/runtime
  root and should be referenced from the issue workpad when an active run pauses.
- Rendered agent prompts are runtime artifacts. They must not be written into
  issue worktrees by default, because prompt scratch files make PR handoff look
  dirty before source changes are ready. Backends receive the rendered prompt on
  stdin and, when a path is needed, through `SHEA_SYMPHONY_PROMPT_PATH` pointing
  at the logs prompt artifact.
- Secrets must not be promoted into repo docs or logged artifacts.

## Cleanup Planning

`clean plan` is the grouped cleanup-planning command. It is dry-run only and reports
worktrees that are safe candidates for operator removal when all of these are
true:

- the tracker state is terminal;
- the linked PR is merged or closed;
- the local worktree branch matches the planned issue branch;
- `git status --porcelain` is clean;
- the worktree path still exists under the configured workspace root.

The command never deletes files:

```bash
cargo run -- clean plan .shea/workflows/shea-symphony.md
cargo run -- clean plan .shea/workflows/shea-symphony.md
```

`clean audit` is also read-only. It classifies configured local artifacts and
workspaces by persistence action:

- `promote_to_repo`: reusable workflow or prompt material that should live in
  repo-owned docs, examples, or workflows.
- `attach_to_tracker`: PR body or workpad drafts that should be represented by a
  pull request, issue comment, or tracker workpad.
- `safe_to_keep`: runtime state, event logs, and review artifacts that are local
  evidence while work is active or recently reviewed.
- `cleanup_candidate`: disposable scratch or terminal clean worktrees eligible
  for a future guarded apply flow.
- `needs_human_decision`: dirty, ambiguous, or non-terminal residue that should
  not be deleted automatically.

```bash
cargo run -- clean audit .shea/workflows/shea-symphony.md
```

Use the report to decide what to remove manually or in a future explicit
write-mode cleanup command. Temp cleanup is not proof that tracker evidence is
complete.
