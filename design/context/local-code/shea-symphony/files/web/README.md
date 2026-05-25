# Shea Symphony Operator Desk

This web app was migrated from the OpenDesign prototype at:

`/Users/chuntengxiao/Library/Application Support/Open Design/namespaces/release-stable/data/projects/65ae20da-8bf8-4e78-b685-98b0fd5de2f6/`

It is a SvelteKit static build with a local Node server that exposes a small
loopback-only API for Shea Symphony CLI commands.

## Run

```sh
cd web
npm run build
npm run serve
```

Open `http://localhost:5173/`.

For an offline smoke/demo mode that does not call GitHub or mutate tracker
state:

```sh
cd web
npm run build
npm run serve:fixture
```

## Live CLI Bridge

The server reads `SHEA_WORKFLOW` when set, otherwise it uses:

`workflows/shea-symphony.md`

Supported UI actions are intentionally allowlisted in `server.mjs`:

- `autopilot plan --json`
- `doctor --json`
- `review status --json`
- `skills status --json`
- `project issue --json`
- `project inspect`
- `gate`
- `project set-state`
- `autopilot loop --once`
- `merge once`
- `project timeline-comment`

Write-mode actions require the UI write toggle; otherwise the server passes
dry-run flags where the CLI supports them.

`SHEA_WEB_FIXTURE=1` returns live-shaped sample data and fixture command output
through the same API routes. Use it for browser QA when GitHub/network access is
unavailable.

## Handoff Files

The OpenDesign reference notes and screenshot are preserved in `handoff/`.
