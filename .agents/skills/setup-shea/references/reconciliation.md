# Target-Owned Reconciliation

Use the pinned release commit and the completed discovery inventory to prepare
one exact, confirmation-gated plan.

## Plan Skills And Markdown

Run the standard Skills CLI in a temporary project-local staging root against
the verified detached checkout, for example:

```text
npx skills add <detached-checkout>/.agents/skills --list
npx skills add <detached-checkout>/.agents/skills --skill <operator-selected-skill> --agent <selected-agent> --copy --yes
```

Let its current supported-agent surface place only the operator-selected normal
Skills for the selected harnesses. Use `--copy`, not its shared symlink mode, so
the staged files are independent. Add the CLI-produced Skill directories to the
target reconciliation plan, but do not transfer its temporary
`skills-lock.json` or other install metadata. After this initial copy, setup
must not invoke `skills check` or `skills update`; local divergence is not
drift. Do not use Shea CLI installation commands or vendor a whole suite
implicitly.

Fetch selected workflows, capability contracts, adapters, prompts, templates,
and workpads from their canonical paths at the same full commit. Do not package
copies under `setup-shea/assets` and do not invent a suite/resource manifest.

## Retire Removed Skills

Inventory every selected harness's project-local Skill root for the exact
legacy directories `shea-symphony-issue-forge-dream` and
`shea-symphony-issue-forge-reflect`. The current release does not install an
alias, wrapper, or deprecated replacement for either name.

Treat each legacy directory as operator-owned. Before proposing removal:

- enumerate every contained path, file type, byte size, and digest;
- use Git's tracked preimage when available to identify local modifications;
- show a focused diff for modified tracked text and the complete current text
  for bounded untracked or otherwise customized text files;
- stop for an operator decision on unreadable, binary, unexpectedly large,
  symlinked, nested-repository, or path-escaping content; and
- state whether Git or a separately confirmed export makes the bytes
  recoverable.

Classify an exact directory tree as `remove_legacy` only after its visible
content and preimage tree digest are in the plan. Deletion requires explicit
confirmation bound to that directory and digest. Re-read the complete preimage
immediately before removal, stop on drift, delete no parent or sibling path,
and verify the exact legacy directory is absent afterward. A declined removal
leaves the legacy directory in place and reports setup as not fully reconciled.

Classify each target path:

- `add`: target path is absent;
- `unchanged`: target bytes exactly equal the staged commit-pinned bytes;
- `conflict_keep`: differing target bytes remain untouched;
- `conflict_replace`: operator explicitly selected the displayed replacement;
- `conflict_manual_merge`: operator explicitly approved a reviewed merged diff.

Without an upstream baseline or upstream-hash registry, a differing existing
file is always a conflict. Age or a newer release is not overwrite authority.

## Confirm Exact Effects

Show one setup plan containing:

- stable tag, full commit, and verified detached-checkout identity;
- selected harnesses and Skills CLI commands;
- every target path, classification, staged digest, and focused diff for edits;
- locally customized files that will be kept;
- workflow/App/runtime-profile changes;
- every machine or external Project effect;
- verification commands and rollback for file writes.

Ask for explicit confirmation bound to those paths, bytes, commands, and
external effects. Separate confirmations when one choice is independent or
high impact. Re-stage and re-confirm if the source identity, target bytes,
selected paths, diff, or external plan changes.

Existing customized `shea-symphony-backlog` and `shea-symphony-improve` files
follow the normal conflict rules. Never replace their Skill or reference text
merely because the stable release is newer or because a legacy Skill is being
removed in the same setup run.

## Apply And Read Back

Apply only confirmed effects. Before each write, prove the target bytes still
match the planned preimage. Use atomic writes for repository files and the
runtime profile. Stop on the first mismatch or failure; do not silently merge.

Re-read every written path and compare it with the confirmed bytes. Re-run the
standard Skills CLI listing for selected project-local Skills and targeted
reads for external Project changes. Report partial application precisely and
leave unrelated operator-owned bytes unchanged.

For confirmed legacy removal, verify only the two exact directories are absent
and that every kept or conflicting Backlog/Improve customization is byte-for-
byte unchanged.
