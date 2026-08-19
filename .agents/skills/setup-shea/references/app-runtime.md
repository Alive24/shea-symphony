# Stable App Runtime

Use the App and embedded Legacy sidecar from the same stable release that owns
the repository resources. Do not distribute, download, or install a standalone
Shea CLI.

## Select And Verify The Native Asset

Read `release-manifest.json`, `SHA256SUMS`, and the Release asset metadata once
from the resolved stable Release. Require all four published assets and require
the manifest tag, semantic version, full source revision, target identities,
package digests, runtime digests, `legacy_cli` role, and
`shea-legacy-cli-v1` compatibility to agree with the pinned release commit.
Require each GitHub asset's `digest` field and the downloaded bytes to match the
corresponding SHA-256 value. Reject missing, extra, partial, duplicate, empty,
or mismatched assets.

Select exactly one supported App package:

| Host | Rust identity | Release asset |
| --- | --- | --- |
| Apple Silicon macOS | `aarch64-apple-darwin` / `macos` / `aarch64` | `Shea-Symphony-App-<tag>-macos-arm64.zip` |
| Windows x64 | `x86_64-pc-windows-msvc` / `windows` / `x86_64` | `Shea-Symphony-App-<tag>-windows-x64-setup.exe` |

Report Linux, Intel macOS, Windows ARM, ambiguous emulation, or an unrecognized
host as unsupported. Do not substitute another target or a standalone binary.

## Reuse A Compatible Installed App

Read `~/.shea-symphony/runtime-discovery.json`; on Windows, resolve `~` through
the operator's user profile. Treat absence as `missing`, parse/schema failure as
`invalid`, a different release as `stale`, target disagreement as
`wrong_platform`, role/compatibility disagreement as `incompatible`, and an
executable or digest mismatch as `tampered`.

For possible reuse, require the record's App/CLI version, full source revision,
target, platform, architecture, role, compatibility, executable SHA-256, and
absolute executable path to match the selected Release manifest. Invoke the
recorded executable directly with `--runtime-info`, compare the live identity,
and recompute its digest. Reuse only a fully matching record; an explicit
protected-2606 `cli_path` is not stable-release App discovery.

## Confirm Installation Or Update

For `missing`, `stale`, `wrong_platform`, `incompatible`, or `tampered`, stage
the verified App package outside the target repository. Show an exact separate
machine-mutation plan containing the stable tag/commit, asset name and digest,
current classification, destination, replacement behavior, visible OS steps,
rollback, and the fact that the first release is unsigned. Obtain confirmation
bound to that plan before downloading to a persistent location, extracting,
installing, replacing, or launching anything.

- On macOS, extract the verified zip to staging and let the operator choose the
  Applications destination. Do not remove quarantine metadata, invoke
  `xattr`/`spctl` bypasses, or suppress Gatekeeper. Guide the operator through a
  visible move and visible first launch.
- On Windows, launch the verified NSIS installer without `/S`, elevation flags,
  response files, or hidden-window options. Let the operator complete or cancel
  its visible per-user installation and any SmartScreen or elevation UI.

Never silently replace or launch an App. If the operator declines or cancels,
leave the installed App and repository unchanged, discard staged bytes, and
report runtime readiness as blocked by that decision.

## Launch And Read Back

Ask the operator to launch the App after installation or update. Wait for the
App to publish runtime discovery, then re-read the record and repeat every
manifest, digest, live `--runtime-info`, role, compatibility, version,
revision, and target check. A missing record after launch remains not ready;
do not invent a path or copy the embedded runtime elsewhere.

Resume workflow, Project, Skill, Markdown, runtime-profile, and no-claim
readiness only after installed runtime verification succeeds. Repeated setup
reuses a compatible App and still preserves all target-owned repository
customizations.
