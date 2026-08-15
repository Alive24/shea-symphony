---
name: setup-shea
description: Install or reconcile a verified Shea Legacy CLI, selected project-local coding-agent skills, and a target-owned Shea workflow contract. Use for first setup, reruns, harness/backend changes, runtime upgrades or rollbacks, and normal no-claim readiness; keep unusual later drift in Shea Doctor.
metadata:
  short-description: Install and reconcile Shea safely
  suite-version: 2026.08.15
---

# Setup Shea

Stay the operator's single conversational setup interface from discovery through
readback. The standard Skills CLI and the bundled controller are tools used by
this skill; do not hand the operator into a second setup wizard.

## Bootstrap

This skill is installable without a Shea checkout. From the target repository,
the standard project-local command is:

```bash
npx skills add https://github.com/Alive24/shea-symphony/tree/main/skills/shea-symphony/suite/setup-shea
```

The Skills CLI detects supported agents and lets the operator choose the
project-local target. Never add `-g`; setup is project-local by default.

## Continuous Flow

1. Resolve the target git root, remote/fork, base branch, GitHub authentication,
   Issues availability, Project owner/number, fields/statuses, existing `.shea`
   surfaces, instructions, toolchain/CI evidence, selected harnesses, and lane
   backend executables. Read existing setup manifests and machine-local profiles.
2. Detect Codex, Claude Code, and Antigravity independently. Let the operator
   choose any available subset. These are interactive skill surfaces, not lane
   transports. Never infer an unattended Antigravity backend from `.agents`.
3. Use Runtime Onboarding reasoning to prepare a credential-free schema-v1
   runtime profile from installed tools and repository requirement sources.
   Respect effective CI `continue-on-error`: record advisory checks separately
   from blocking verification and preserve the exact working directory,
   architecture, bounded environment overlay, and process boundary.
4. Prefer a compatible explicit `cli_path`, then validated App discovery. Only
   when neither works, resolve a pinned `legacy-v*` release and matching target
   artifact. Reject missing checksums, digest mismatch, malformed identity,
   `temporal_worker`, incompatible contracts, target/architecture/version/source
   mismatch, and unavailable versions.
5. Build a schema-v1 request outside committed repository state and run the
   bundled controller's read-only `plan`. The plan must show source revision,
   normal skills, harness targets, link/copy ownership, runtime URL/digest/target
   and versioned user-local path, repository file classifications, verification,
   conflicts, staging boundary, and every proposed Project write.
6. Ask one explicit confirmation for the exact plan id before binary download,
   machine-local installation, standard Skills CLI writes, or repository writes.
   Ask a second, separate confirmation for the exact Project plan id before any
   external Project field creation. Never translate a broad “continue” into
   approval for a changed plan; recompute and show the new id.
7. Apply through the controller. It installs no executable in the repository,
   never mutates shell startup files, pins the resolved executable in ignored
   `.shea/app-profile.local.json`, uses only the standard Skills CLI for skill
   install/update/remove, preserves operator-owned content, and confines drafts
   to the ignored marker-bearing `.shea/local/setup/<run-id>/` namespace.
8. Read back repository/tracker identity, Project schema, harnesses and skills,
   runtime source/path/version/identity, backends, runtime-profile readiness,
   blocking/advisory verification, managed files, conflicts, setup-staging
   cleanup, and first-issue dispatch readiness. Run setup again unchanged and
   require a no-op plan with no unnecessary network or machine/external writes.

Controller commands (resolve this skill's own directory; do not assume a Shea
checkout):

```bash
node scripts/setup-shea.mjs plan --repo <target> --request <request.json>
node scripts/setup-shea.mjs apply --repo <target> --request <request.json> --confirm <plan-id>
node scripts/setup-shea.mjs project-apply --repo <target> --request <request.json> --confirm <project-plan-id>
```

## Ownership And Conflicts

- Classify every action as `create`, `update`, `remove`, `no-op`, `conflict`, or
  operator-owned. A second unchanged run is a true no-op.
- On harness removal, remove only the named normal Shea set through the Skills
  CLI, then reinstall it for the remaining selected harnesses. Preserve every
  unrelated project skill.
- Preserve edits outside the `.gitignore` managed region and every unrelated
  `.shea/local/` namespace. Remove only the current exact marker-bearing setup
  run after success or handled failure; an interrupted marked run may be resumed
  only after its completed steps are revalidated.
- Never overwrite a conflicting managed file. Show the focused path/reason and
  ask the operator whether to preserve, adopt, or replace it, then create a new
  plan. Leave unusual semantic drift to `$shea-symphony-doctor`.
- Keep credentials, tokens, inherited environments, runtime binaries, absolute
  executable paths, runtime profiles, setup staging, logs, worktrees, and
  sessions out of committed files.

## No-Claim Hard Boundary

Setup and readiness may run `--runtime-info`, workflow validation, runtime
profile readiness, skill status, backend probes, Project schema reads, and
baseline verification. They must never call Main/Review/Merge claims or loops,
transition an issue, recover an issue run, create an implementation worktree, or
use a real issue as setup UAT. End with readiness evidence, not dispatch.

Dream and HALO research are explicit additions. The normal install set is
`setup-shea`, Runtime Onboarding, Doctor, Issue Forge, Investigate, Reflect,
Manual Main, Manual Review, Human Review, and Manual Merge.
