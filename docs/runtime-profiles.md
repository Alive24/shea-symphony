# Repository Runtime Profiles

Shea separates two local profile contracts:

- `.shea/app-profile.json` tells the App which workflow and CLI to use.
- `.shea/runtime-profile.json` tells Main which already-installed repository
  execution environment was selected and how to verify it safely.

The runtime profile is machine-local, credential-free, and Git-ignored. It is
produced by `setup-shea` after repository-specific discovery and explicit
operator confirmation. The same setup entrypoint owns initial onboarding,
incomplete setup, and later environment-drift reconciliation. Shea core never
discovers Node, pnpm, Python, Rust, Conda, Docker, Nix, or another ecosystem
itself.

## Workflow Configuration

For a workflow at `.shea/workflows/target.md`, the default profile path is
`.shea/runtime-profile.json`:

```yaml
runtime_profile:
  required: true
  timeout_ms: 10000
```

An explicit `path` is resolved relative to the workflow directory. Existing
workflows remain compatible because `required` defaults to `false`; an absent
optional profile reports `skipped:not_configured` and leaves existing
workflow-defined `profiles.entries` behavior unchanged. Once a target adopts
runtime onboarding, set `required: true` so missing or invalid readiness fails
closed before Main claim.

## Schema Version 1

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
      "path": "package.json",
      "git_blob": "0123456789012345678901234567890123456789"
    }
  ],
  "tools": [
    {
      "id": "runtime",
      "executable": "/absolute/path/to/runtime",
      "observed_version": "24.18.1",
      "version_args": ["--version"]
    }
  ],
  "environment": {
    "PATH": "/bounded/runtime/path:/usr/bin:/bin"
  }
}
```

`requirement_sources` are repository-relative paths fingerprinted with
`git hash-object`. A changed manifest, lockfile, toolchain file, CI workflow,
or other selected source invalidates readiness and asks the operator to rerun
onboarding. Each tool probe executes the absolute binary directly with bounded
argv; Shea does not evaluate a probe through a shell. Schema v1 accepts one of
`--version`, `-V`, `-v`, `version`, or `-version` as the complete probe argv.

The environment overlay accepts at most 64 conventional environment keys and
rejects credential-bearing names, credential-like values, control characters,
and reserved `SHEA_SYMPHONY_*` variables. Shea adds the profile identity itself.
Known process-injection variables are also rejected. The profile never stores
a complete ambient process environment.

## Main Ordering and Evidence

For live Main work, Shea performs these steps in order:

1. inspect tracker eligibility and plan the issue handoff;
2. prepare or reuse the canonical issue worktree locally;
3. load and validate the machine-local profile;
4. fingerprint requirement sources and run direct tool probes in that exact
   worktree;
5. on failure, write only local diagnostic evidence under the configured logs
   root and leave claim, Status, and backend untouched;
6. on success, re-read tracker status, dependencies, assignee, and lane claim;
7. write the Main claim and `In Progress` Status;
8. apply the runtime overlay to Main backend execution;
9. apply the same runtime overlay and identity to configured handoff
   verification commands.

Successful profile identity, worktree, matched sources, and observed tool
versions are safe to record in Main runtime/workpad evidence. Environment
values and probe arguments are not included in that evidence.

The first slice does not apply repository runtime profiles to Review or Merge.
Doctor may report a profile problem, but routes discovery, environment
selection, and confirmed profile writes back to `setup-shea`.

## Operator Readiness Check

Run the profiles command from the repository or exact issue worktree that Shea
will execute:

```bash
shea-symphony profiles /absolute/path/to/.shea/workflows/target.md
```

The output reports the profile path, status, id, checked workspace, matched
source fingerprints, and observed tool versions without environment values.

## FailureReport #29 UAT Boundary

The #513 acceptance procedure is operator-owned. Reuse the existing
FailureReport #29 worktree and commit `288f17d`; do not relaunch implementation.
After confirming the proposed already-installed Node 24.18.1 environment, the
operator writes the ignored profile, verifies readiness in the adopted issue
worktree, then runs `pnpm build`, `pnpm check`, `pnpm test`, and
`pnpm format:check`. Pushing that branch, creating or reusing its PR, and
handing #29 to Agent Review remain actions in the normal operator-owned
FailureReport workflow, not automated side effects of Shea issue #513.
