# Setup Shea

`setup-shea` is the single conversational entry point for installing and
reconciling Shea Symphony in another repository. It coordinates the public
[Skills CLI](https://github.com/vercel-labs/skills), a verified standalone
Legacy CLI release, target-owned repository contracts, and no-claim readiness.
It is not an App wizard and never dispatches an issue.

## First Action

From the target repository, install only the bootstrap skill at project scope:

```sh
npx skills add https://github.com/Alive24/shea-symphony/tree/main/skills/shea-symphony/suite/setup-shea
```

The Skills CLI detects compatible agents and prompts for the project-local
destination. Project scope is the default. The setup skill later invokes the
same CLI against the exact GitHub commit archive for every selected normal
skill, with explicit names, full-depth discovery, and bounded trusted-archive
limits. This avoids treating a commit SHA as a branch while retaining an
immutable source; no Shea checkout or custom installer is required.

## Discovery And Selection

Before proposing a write, setup discovers:

- git root, canonical remote or fork, base branch, GitHub authentication, and
  Issues availability;
- GitHub Project owner/number, required Status options, and Main/Review/Merging
  claim fields;
- existing `.shea`, instruction, workflow, prompt, template, workpad, handoff,
  App-profile, skill, runtime-profile, and setup-manifest files;
- repository toolchain and effective blocking/advisory CI policy;
- compatible explicit/App-discovered Legacy runtimes and the pinned fallback
  release for the current native target;
- available Codex, Claude Code, and Antigravity interactive skill harnesses;
- supported Main, Review, and Merge transports available on the machine.

Interactive harness selection and unattended lane backends are independent.
Antigravity may receive project-local skills, but its skill directory is never
treated as a headless lane transport. Main and Merge use Codex app-server or
Claude Code stream-json; Review uses an explicitly supported read-only backend.

## Plan And Confirm Boundaries

The bundled controller's `plan` command is read-only. It classifies each action
as `create`, `update`, `remove`, `no-op`, `conflict`, or operator-owned and
prints one stable plan id. The visible plan includes:

- suite version/revision and the exact standard Skills CLI source/targets;
- runtime source, version, revision, target, architecture, URL, checksum, and
  versioned user-local install path;
- managed and machine-local files, focused conflicts, and staging cleanup;
- lane backends plus blocking and advisory baseline verification;
- every proposed GitHub Project field/status write.

Binary download/install, project-local skill changes, machine-local files, and
repository changes require exact confirmation of that plan id. GitHub Project
writes require a separate confirmation of the Project plan id. A recomputed or
changed plan invalidates earlier confirmation.

The controller lives inside the installed skill:

```sh
node scripts/setup-shea.mjs plan --repo <target> --request <request.json>
node scripts/setup-shea.mjs apply --repo <target> --request <request.json> --confirm <plan-id>
node scripts/setup-shea.mjs project-apply --repo <target> --request <request.json> --confirm <project-plan-id>
```

The skill prepares the request from the operator's choices and repository
evidence. Operators should review it like any other machine-change plan; it must
not contain credentials or a copied parent environment.

## Skill Installation And Updates

The normal set is `setup-shea`, Runtime Onboarding, Doctor, Issue Forge,
Investigate, Reflect, Manual Main, Manual Review, Human Review, and Manual
Merge. Dream and HALO research are explicit additions.

After confirmation, the controller invokes the standard CLI with the pinned
commit archive and selected agents. It raises the CLI's archive limits only for
this exact trusted GitHub source, to 128 MiB download, 256 MiB extracted content,
and 10,000 files. Current project-local paths are owned by the
standard CLI: `.agents/skills` for Codex and Antigravity, and `.claude/skills`
for Claude Code. Setup does not copy skills itself or maintain an alternate path
registry. `npx skills update -p -y` and `npx skills remove` remain the update and
removal mechanisms.

An unchanged rerun verifies visible skill files and the stored source revision,
then skips the Skills CLI entirely. Adding/removing a harness or changing the
suite revision is a visible managed action. Because standard project installs
share a canonical `.agents/skills` store, removing a harness deletes only the
named normal Shea set, then reinstalls that set for the remaining selected
harnesses; unrelated project skills are preserved.

## Runtime Install, Upgrade, And Rollback

Setup preserves a compatible operator-supplied Legacy CLI path. Otherwise it
uses a validated App discovery record or proposes a pinned `legacy-v*` release.
See [Legacy Runtime Distribution](legacy-runtime-distribution.md) for the native
support matrix and trust checks.

Release binaries live under
`~/.local/share/shea-symphony/runtimes/<version>/<target>/`. Prior versions are
not deleted. To upgrade or roll back, rerun setup with a different pinned release
tag, review the new identity/checksum/path, and confirm. Only ignored
`.shea/app-profile.local.json` changes to select the resolved absolute path;
committed files never contain it.

## Managed Repository Contract

Setup manages marker-bearing repository identity and lane configuration under
`.shea/workflows`, short lane prompts, workpad templates, Human Review handoff,
concise `.shea/README.md`, and `.shea/setup.json`. The manifest records selected
harnesses, suite revision, credential-free runtime identity, backend selection,
verification policy, and hashes for full-file managed surfaces.

`.gitignore` uses a stable managed region and preserves all operator-owned lines.
Committed baseline files may contain repository/Project identity and executable
names, but no machine paths or secrets.

Ignored machine-local state includes:

- `.shea/app-profile.local.json` and `.shea/runtime-profile.json`;
- `.shea/local/setup/`, logs, artifacts, worktrees, and sessions;
- downloaded runtime binaries and full inherited environments.

If a managed file differs from its recorded hash, setup reports a focused
conflict and preserves it. The operator chooses preserve, adopt, or replace and
then reviews a new plan. Normal reruns do not need Doctor; use Doctor for unusual
post-setup semantic drift.

## Staging, Recovery, And Cleanup

Repository drafts use only `.shea/local/setup/<run-id>/`. Setup first installs
and verifies the ignore rule, then creates a marker. An interrupted marked run
is safely restarted only after runtime and file state are revalidated. Normal
success and handled failure remove the current run. Cleanup never traverses or
deletes a sibling producer's `.shea/local` namespace or an unmarked directory.

The readiness report records setup namespace size and git status before/after;
historical setup runs are excluded from prompt context.

## No-Claim Readiness

After applying approved changes, setup verifies the installed executable again,
parses and smoke-renders the workflow/prompts/templates, checks Project schema
readback, validates selected skills and backend executables, resolves the
machine-local runtime profile, and runs baseline verification with the same
working directory and bounded environment used by later Main execution.

Readiness may run `--runtime-info`, `validate`, `profiles`, `skills status`,
read-only GitHub/Project queries, and repository checks. It does not call lane
claims/loops, transition a real issue, recover an issue run, or create an issue
worktree. A second immediate plan must contain only `no-op` actions and leave
git status plus unrelated `.shea/local` content unchanged.

The final readback includes repository/Project identity and schema, harnesses and
visible skills, runtime source/path/version/identity, backends, runtime-profile
status, verification results, managed files, conflicts, staging cleanup, and
whether a first issue is ready for dispatch.
