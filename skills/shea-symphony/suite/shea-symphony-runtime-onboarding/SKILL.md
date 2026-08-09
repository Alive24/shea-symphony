---
name: shea-symphony-runtime-onboarding
description: Discover a target repository's execution requirements, propose a credential-free machine-local Shea runtime profile, and write it only after operator confirmation.
metadata:
  short-description: Prepare repository runtime readiness
  suite-version: 2026.08.07
---

# Shea Symphony Runtime Onboarding

Produce the repository-specific input consumed by Shea core. Discovery and
environment selection belong here; Shea core remains ecosystem-agnostic and
only validates the resulting bounded profile.

This skill is an operator workflow. It may inspect repositories and installed
tools without confirmation. It must stop after presenting a dry-run proposal
before it writes `.shea/runtime-profile.json`, installs anything, changes shell
startup files, selects a different execution environment, or makes another
machine-local change.

## Bind the Target Repository

1. Resolve the target repository root with `git rev-parse --show-toplevel`.
2. Read the target's `AGENTS.md` files that govern the root and relevant
   packages.
3. Prefer `.shea/app-profile.local.json` over `.shea/app-profile.json` only for
   locating the target workflow and CLI. App profile selection is not runtime
   readiness and must never be edited by this skill.
4. Resolve the workflow's tracker repository identity and the runtime-profile
   path. The default is `.shea/runtime-profile.json` under the ignored target
   runtime.
5. Confirm `.shea/` or the exact runtime-profile path is ignored locally. Do
   not add a tracked credential or machine-path file.

## Discover Requirements

Inspect only repository-owned evidence that can affect execution:

- applicable agent instructions;
- package, workspace, and dependency manifests;
- lockfiles;
- toolchain and version-manager files;
- CI workflows and development containers;
- configured Shea verification commands;
- build, test, lint, formatting, and contributor documentation.

Record every source path and compute its current Git blob fingerprint with:

```bash
git -C <target-root> hash-object -- <repository-relative-path>
```

If sources disagree about a required tool or version, report the conflict and
stop. Do not silently choose one. Supporting every ecosystem is not required;
state clearly when the available evidence cannot be represented safely.

## Resolve Existing Tools

Prefer an already-installed compatible executable. For each required tool:

1. Discover candidates without changing the machine.
2. Resolve the selected candidate to an absolute executable path.
3. Run one cheap, non-destructive direct version probe.
4. Record the short observed version text and the direct argv used by the
   probe.
5. Reject shell activation commands, compound shell programs, installers, or
   probes that require credentials or network access.

Schema v1 accepts one conventional direct version argument: `--version`,
`-V`, `-v`, `version`, or `-version`. If a tool cannot report compatibility
through one of those probes, report it as unsupported by the first slice.

If no compatible candidate exists, report the missing requirement and ask the
operator how to proceed. Do not install or mutate shell/system configuration.

## Profile Contract

Prepare a JSON proposal with exactly this schema-v1 shape:

```json
{
  "schema_version": 1,
  "profile_id": "repository-compatible-runtime",
  "generated_at": "2026-08-07T00:00:00Z",
  "repository": {
    "id": "owner/repository"
  },
  "requirement_sources": [
    {
      "path": "relative/manifest-or-toolchain-file",
      "git_blob": "40-character-git-blob-digest"
    }
  ],
  "tools": [
    {
      "id": "repository-tool-name",
      "executable": "/absolute/path/to/already-installed/tool",
      "observed_version": "short version text",
      "version_args": ["--version"]
    }
  ],
  "environment": {
    "PATH": "/bounded/tool/path:/usr/bin:/bin"
  }
}
```

Keep the environment overlay minimal. Never copy the complete parent process
environment. Do not record tokens, passwords, cookies, authorization headers,
API keys, private keys, credentials, or `SHEA_SYMPHONY_*` control variables.
Do not put secrets in probe argv. Prefer `PATH` plus only repository-required,
non-secret variables.
Do not propose process-injection variables such as `LD_*`, `DYLD_*`,
`GIT_CONFIG*`, `BASH_ENV`, `NODE_OPTIONS`, runtime startup hooks, or shell
prompt hooks.

Requirement paths must be repository-relative and cannot contain `..`.
Executables must be absolute paths. Use one entry per distinct tool and source.

## Confirmation and Write

Before writing, show the operator:

- target repository and output path;
- requirement sources and fingerprints;
- each selected executable, observed version, and probe argv;
- environment variable names, with values shown only when plainly non-secret;
- conflicts, assumptions, and why the selected environment is compatible;
- confirmation that no installation or shell/system change is planned.

Ask one explicit confirmation to write the proposed profile. If the operator
does not confirm, preserve the proposal outside the repository and stop.

After confirmation:

1. Write exactly one `.shea/runtime-profile.json` atomically.
2. Confirm the file is ignored with `git check-ignore` or the repository's
   local exclude evidence.
3. Re-read the file and scan both keys and values for credential-bearing data.
4. Run Shea's runtime-profile readiness surface against the target repository
   or exact issue worktree.
5. Report the profile id, schema, matched requirement sources, selected tool
   versions, and readiness result without exposing environment values.

## FailureReport #29 Operator UAT

When the operator explicitly performs #513's UAT against FailureReport:

- reuse the existing issue #29 worktree and commit `288f17d`;
- do not relaunch or reimplement Main;
- confirm repository evidence requires Node >=24 and pnpm 10.15.0;
- propose the already-installed Node 24.18.1 environment when it remains
  compatible;
- write the profile only after operator confirmation;
- verify readiness in the adopted #29 worktree before any new Main claim;
- run `pnpm build`, `pnpm check`, `pnpm test`, and `pnpm format:check` through
  the normal operator-owned workflow;
- leave pushing the branch, PR publication, and #29 Agent Review handoff to the
  operator-owned FailureReport run.

This UAT section does not authorize an implementation agent working on Shea
Symphony to mutate the FailureReport repository.
