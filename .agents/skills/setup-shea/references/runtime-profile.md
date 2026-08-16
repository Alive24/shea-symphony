# Runtime Profile Discovery And Confirmation

Produce the repository-specific, credential-free input consumed by Shea core.
Discovery and environment selection remain agent-led; core only validates the
bounded result.

## Bind The Profile

Resolve the workflow's `runtime_profile.path`; default to
`.shea/runtime-profile.json` under the target runtime. Confirm `.shea/` or the
exact profile path is Git-ignored. Never add a tracked machine-path or
credential file.

Inspect only repository-owned evidence that can affect execution: applicable
instructions, manifests, lockfiles, toolchain/version-manager files, CI,
development containers, contributor docs, and configured verification. Record
each selected relative source path and its current Git blob fingerprint with
`git hash-object`. Stop when sources disagree about a required tool or version.

## Resolve Existing Tools

For each requirement, prefer an already-installed compatible executable:

1. Discover candidates without changing the machine.
2. Resolve the selected candidate to an absolute path.
3. Run one cheap direct version probe.
4. Record short observed version text and the direct argv.
5. Reject shell activation, compound shell programs, installers, credentialed
   probes, or probes that require network access.

Schema v1 accepts one conventional direct version argument: `--version`, `-V`,
`-v`, `version`, or `-version`. Report an unsupported or missing requirement;
do not install tools or change shell/system configuration.

## Schema V1

Prepare exactly this shape:

```json
{
  "schema_version": 1,
  "profile_id": "repository-compatible-runtime",
  "generated_at": "2026-08-16T00:00:00Z",
  "repository": { "id": "owner/repository" },
  "requirement_sources": [
    { "path": "relative/manifest", "git_blob": "40-character-git-blob-digest" }
  ],
  "tools": [
    {
      "id": "repository-tool-name",
      "executable": "/absolute/path/to/installed/tool",
      "observed_version": "short version text",
      "version_args": ["--version"]
    }
  ],
  "environment": { "PATH": "/bounded/tool/path:/usr/bin:/bin" }
}
```

Requirement paths are repository-relative without `..`; executables are
absolute. Keep the environment minimal. Never copy the ambient environment or
record tokens, passwords, cookies, authorization headers, API/private keys,
credentials, secret values, or `SHEA_SYMPHONY_*` controls. Reject process-
injection variables such as `LD_*`, `DYLD_*`, `GIT_CONFIG*`, `BASH_ENV`,
`NODE_OPTIONS`, startup hooks, and prompt hooks.

## Confirm, Write, Verify

Before writing, show the target/path, source fingerprints, executables,
observed versions, probe argv, non-secret environment names/values, conflicts,
assumptions, and confirmation that no install or shell/system change is
planned. Obtain explicit confirmation for the exact profile bytes.

Write atomically, confirm the path remains ignored, re-read it, and scan keys
and values for credentials. Run the pinned Shea runtime-profile readiness
surface in the exact target/worktree. Report profile id/schema, matched sources,
tool versions, and readiness without exposing environment values.

On later drift, Doctor diagnoses and routes repository discovery or profile
reconciliation back to `setup-shea`; Doctor does not select the environment or
rewrite this profile.
