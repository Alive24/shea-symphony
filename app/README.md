# Shea Symphony App

This app was migrated from the OpenDesign prototype at:

`/Users/chuntengxiao/Library/Application Support/Open Design/namespaces/release-stable/data/projects/65ae20da-8bf8-4e78-b685-98b0fd5de2f6/`

It is a Tauri v2 desktop shell with a Vite + Svelte foreground surface for Shea
Symphony. The runnable surface is the first-screen Operator Desk: Human Todo and
Lane Board. Tauri owns the local Autoloop process, launched through `autopilot loop`, and uses allowlisted
Shea Symphony CLI commands for readback; there is no runtime Node bridge.

During the 2607 transition, those legacy operator commands come from the
`shea-symphony-legacy` sidecar built from the same `main` revision as the App.
The default `shea-symphony` executable remains the Temporal worker.

## Run

```sh
cd app
npm run tauri -- dev
```

For browser-only visual QA, run Vite directly. This mode uses fixture data and
does not control the local autoloop process:

```sh
cd app
npm run dev
```

Open the Vite URL printed by the command.

## Bundle The Legacy Sidecar

Release bundles must stage a target-specific Legacy executable before Tauri
builds the App:

```sh
cd app
npm run bundle:legacy
```

The staging script builds `shea-symphony-legacy` for the active Rust target,
checks its machine-readable role and source revision, and places it under
`src-tauri/binaries/` using Tauri's sidecar naming convention. The command
builds the supported local Tauri App bundle without an installer, Developer ID
signing or notarization. Local macOS bundles receive a verified ad-hoc resource
seal; a linker-signed executable alone is not a sealed App bundle. Existing
valid signatures are retained. No quarantine or Gatekeeper settings are changed.
The command fails clearly when staging or bundle verification fails.

## Legacy Workspace And Review Integration

Select a target in Settings or launch with `--workdir /absolute/repository`.
The App resolves the target's `.shea/app-profile.json`, then its ignored
`.shea/app-profile.local.json`; repository values override saved UI defaults.
Workflow paths must stay inside the selected repository. The same validated
`.shea/runtime-profile.json` overlay reaches CLI reads, GitHub reads and lane
processes. Write commands additionally verify requirement fingerprints and tool
versions before launch. No shell startup file or Temporal service is required.

Use **Check App setup** in Settings to validate the Legacy identity, recovery
command, local profile and all three App handoff prompts. This is local setup
evidence, not tracker eligibility or authorization to launch a lane.

Each Issue detail has an Independent Review panel. **Prepare Review** performs
the guarded CLI dry run; **Confirm and start Review** starts one independent
review for that Issue. **Prepare recovery** and **Confirm recovery** resume a
captured terminal result without another backend. Previews expire after five
minutes, cannot be reused, and are invalidated by workspace/configuration changes.
The CLI remains the owner of claims, exclusion, freshness and state-last writes.

Publication completion requires a final durable receipt. A terminal backend job
without one remains unverified; malformed, superseded or incomplete running
receipts route to Doctor. Merely moving an Issue to Agent Review never launches
it. All Issue actions are explicit; opening the App does not start Autoloop.

Stable packages are produced only by `.github/workflows/release.yml`: an Apple
Silicon macOS App zip and a Windows x64 NSIS installer. The workflow verifies
the embedded sidecar from each native package, aggregates checksums and release
metadata, creates a draft Release, downloads every asset for readback, and only
then publishes it as stable/latest. See `docs/legacy-runtime-distribution.md`
and the versioned notes under `docs/releases/`.

## Live CLI Bridge

The desktop bridge uses:

`.shea/workflows/shea-symphony.md`

Tauri commands are intentionally allowlisted:

- `start_autoloop`
- `stop_autoloop`
- `get_loop_state`
- `get_runtime_snapshot`
- `get_operator_overview`
- `get_read_surface`

The read surfaces call the Shea Symphony CLI directly from Rust. Tauri does not
perform direct `git` or `gh` state reads for the Operator Desk.

CLI resolution is deterministic:

1. an explicit workspace `cli_path`;
2. the validated installed Legacy runtime discovery record;
3. in debug builds only, `cargo run --bin shea-symphony-legacy` from the engine
   checkout.

An explicit path may select an unmarked protected-2606 binary as a temporary
operator escape hatch. Automatic discovery never accepts an unmarked binary or
the `temporal_worker` role. On startup, a packaged App validates its bundled
sidecar and atomically publishes a credential-free discovery record at
`~/.shea-symphony/runtime-discovery.json` (or
`SHEA_SYMPHONY_RUNTIME_DISCOVERY_PATH`). The record includes the executable
path, SHA-256 digest, role, compatibility contract, version, revision, target,
platform, and architecture. This integrity check detects stale or mismatched
local artifacts; it is not publisher signing.

Browser preview returns live-shaped fixture data. Use it for visual QA when the
desktop shell is not running.

## UI Structure

The runnable UI intentionally keeps only:

- `Human Todo`
- `Lane Board`
