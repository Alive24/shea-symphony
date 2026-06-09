# Shea Symphony App

This app was migrated from the OpenDesign prototype at:

`/Users/chuntengxiao/Library/Application Support/Open Design/namespaces/release-stable/data/projects/65ae20da-8bf8-4e78-b685-98b0fd5de2f6/`

It is a Tauri v2 desktop shell with a Vite + Svelte foreground surface for Shea
Symphony. The runnable surface is the first-screen Operator Desk: Human Todo and
Lane Board. Tauri owns the local Autoloop process, launched through `autopilot loop`, and uses allowlisted
Shea Symphony CLI commands for readback; there is no runtime Node bridge.

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

## Live CLI Bridge

The desktop bridge uses:

`workflows/shea-symphony.md`

Tauri commands are intentionally allowlisted:

- `start_autoloop`
- `stop_autoloop`
- `get_loop_state`
- `get_runtime_snapshot`
- `get_operator_overview`
- `get_read_surface`

The read surfaces call the Shea Symphony CLI directly from Rust. Tauri does not
perform direct `git` or `gh` state reads for the Operator Desk.

Browser preview returns live-shaped fixture data. Use it for visual QA when the
desktop shell is not running.

## UI Structure

The runnable UI intentionally keeps only:

- `Human Todo`
- `Lane Board`

## Handoff Files

The OpenDesign reference notes and screenshot are preserved in `handoff/`.
