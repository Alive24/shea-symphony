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

## Standalone Release Pipeline

Tags named `legacy-v<crate-version>` run
`.github/workflows/release-legacy-cli.yml`. The workflow builds and executes the
Legacy binary natively on the documented first-slice matrix:

| Target | GitHub runner | Archive support |
| --- | --- | --- |
| `aarch64-apple-darwin` | `macos-15` | supported |
| `x86_64-apple-darwin` | `macos-15-intel` | supported while GitHub provides the Intel runner |
| `x86_64-unknown-linux-gnu` | `ubuntu-24.04` | supported |

Windows, Linux ARM, code signing, notarization, App installers, and OS package
managers are not claimed by this first slice. A target is supported only when a
native runner built it and executed its `--runtime-info` output.

`scripts/package-legacy-release.py` checks schema, `legacy_cli` role,
`shea-legacy-cli-v1` compatibility, target, platform, architecture, crate
version, release tag, and exact source revision before producing a deterministic
archive. The release also contains `SHA256SUMS` and `legacy-release.json`; the
latter repeats the pinned release revision and the verified identity/checksum of
every target artifact. Publication fails when any matrix entry or identity is
missing.

For a native local archive smoke:

```sh
target="$(rustc -vV | sed -n 's/^host: //p')"
revision="$(git rev-parse HEAD)"
version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)"
SHEA_SOURCE_REVISION="$revision" cargo build --locked --release \
  --bin shea-symphony-legacy --target "$target"
python3 scripts/package-legacy-release.py \
  --binary "target/$target/release/shea-symphony-legacy" \
  --target "$target" \
  --release-tag "legacy-v$version" \
  --source-revision "$revision" \
  --output-dir dist
```

## App Bundle Pipeline

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

## Setup Installation And Trust

`setup-shea` first preserves a compatible explicit `cli_path`, then checks a
validated App discovery record. Only when neither is suitable does it propose a
pinned GitHub Release download. The visible plan includes release tag, source
revision, target, architecture, archive URL, digest, and versioned user-local
destination. Download and installation require confirmation.

The executable is installed under
`~/.local/share/shea-symphony/runtimes/<version>/<target>/` rather than the
target repository. Existing versions remain available for rollback. Setup never
tracks `latest`, edits shell startup files, or stores the runtime in `.shea`.
The exact resolved executable path is recorded only in ignored
`.shea/app-profile.local.json`.

Installation fails closed unless all of the following agree:

1. `legacy-release.json` selects the requested target and release revision;
2. `SHA256SUMS` authenticates the exact archive digest repeated by that
   metadata;
3. the downloaded archive digest matches its 64-character SHA-256 value;
4. the archive contains only the expected `shea-symphony-legacy` executable;
5. the extracted and installed executable each report schema 1, `legacy_cli`,
   `shea-legacy-cli-v1`, and the expected version, revision, target, platform,
   and architecture.

Missing releases/checksums, wrong digests, malformed JSON, stale revisions,
Temporal-role binaries, and mismatched targets or architectures are rejected.
Checksums and embedded identity are integrity/compatibility evidence, not code
signatures; operators who require publisher signing must wait for a later
distribution slice.

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
