# Legacy Runtime Distribution

## Purpose

Shea Symphony temporarily ships two executables from the same `main` revision:

- `shea-symphony`, the canonical Temporal worker;
- `shea-symphony-legacy`, the compatibility CLI used by current App operator
  surfaces.

The split lets the App move off the protected `2606-MVP` build without changing
the default runtime role or turning the old command graph into a second 2607
architecture.

## Identity Contract

Each executable accepts exactly `--runtime-info` and emits versioned JSON with:

- schema version and binary role;
- CLI package version and source revision;
- Rust target triple, platform, and architecture;
- a versioned compatibility contract.

The Temporal worker reports `temporal_worker` and
`shea-temporal-worker-v1`. The Legacy CLI reports `legacy_cli` and
`shea-legacy-cli-v1`. This output is credential-free build metadata. It is used
for local role and integrity checks, not as a code-signing claim.

## Bundle Pipeline

Run the release pipeline from `app/`:

```sh
npm run bundle:legacy
```

`scripts/stage-legacy-sidecar.sh` resolves the Rust target, builds the Legacy
binary with the root lockfile, stages the target-specific artifact under
`app/src-tauri/binaries/`, and verifies that its role, compatibility contract,
and embedded source revision match the checkout. The Tauri build then requires
and embeds that staged sidecar in the supported local App bundle; installer,
signing, and notarization flows remain out of scope. Generated binaries are
ignored by Git.

Use `scripts/stage-legacy-sidecar.sh --check` to test for the expected staged
artifact without building it. Setting `SHEA_LEGACY_SIDECAR_TARGET` makes target
selection explicit for cross-target packaging.

## Discovery And Resolution

At packaged App startup, the Tauri backend locates its bundled sidecar,
validates the exact Legacy role and same-build identity, computes its SHA-256
digest, and atomically publishes:

```text
~/.shea-symphony/runtime-discovery.json
```

Tests and controlled installs can override that path with
`SHEA_SYMPHONY_RUNTIME_DISCOVERY_PATH`; packaging can provide an explicit
sidecar path through `SHEA_SYMPHONY_BUNDLED_CLI_PATH`.

The discovery record contains the executable path, digest, App and CLI
versions, source revision, target, platform, architecture, role, and
compatibility contract. Every automatic resolution rechecks the digest,
executable identity, and record metadata. A missing, stale, mismatched,
unmarked, tampered, or Temporal-role binary fails closed.

The App resolves commands in this order:

1. workspace `cli_path`;
2. validated installed discovery;
3. debug-only cargo runner for `shea-symphony-legacy`.

Explicit `cli_path` is the only compatibility exception: an operator may point
it at an unmarked protected-2606 binary while completing migration. If an
explicit binary does expose identity, it must be the Legacy role. Release
builds never silently fall back to Cargo or an unmarked discovery record.

## Retirement

Stop creating new protected-2606 App/CLI bootstrap builds after the `main`
sidecar bundle is release-validated, current dogfood installs use it, and no
2606-only operational blocker remains. Retain the protected branch as a
recovery and behavior oracle until the Temporal product path covers the current
operator surfaces and bootstrap-retirement work is complete.

Remove `shea-symphony-legacy`, its discovery record, and the App sidecar only
when the App no longer calls the legacy command graph and recovery procedures
no longer depend on that executable. That removal must not delete the protected
history or its acceptance evidence.
