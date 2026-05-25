# Shea Symphony Operator Desk

This web app was migrated from the OpenDesign prototype at:

`/Users/chuntengxiao/Library/Application Support/Open Design/namespaces/release-stable/data/projects/65ae20da-8bf8-4e78-b685-98b0fd5de2f6/`

It is a visualization-first SvelteKit cockpit for Shea Symphony. The primary
surface shows lane posture, parked human work, skill handoffs, and evidence
signals. A local Node server exposes a small loopback-only API for diagnostics
and exact CLI previews, but day-to-day operations are still expected to happen
through chat Skills.

## Run

```sh
cd web
npm run build
npm run serve
```

Open `http://localhost:5173/`.

`npm run serve` first tries `127.0.0.1:5173`. If that bind target is blocked or
already used, it will try `localhost`, `0.0.0.0`, and a small range of following
ports, then print the actual URL. Set `HOST=...`, `PORT=...`, or
`SHEA_WEB_PORT_FALLBACKS=...` when you need to force or widen that behavior.

For a background server that survives Codex session restarts and does not depend
on the current agent getting loopback bind permission, install the user LaunchAgent:

```sh
cd web
npm run autostart:install
npm run autostart:status
```

It serves `http://127.0.0.1:5173/` through macOS `launchd`, restarts on failure,
and writes logs to `~/Library/Logs/SheaSymphony/`. Remove it with
`npm run autostart:uninstall`.

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

`GET /api/health` reports local web readiness without invoking GitHub or the
Shea CLI: build presence, workflow path, fixture mode, CLI mode, and bind
address. Use it to separate Web server/setup issues from live tracker issues.

## UI Structure

Routes:

- `/`: operator desk with attention queue, workflow map, evidence, and lane posture.
- `/lanes`: lane posture, state pressure, boundaries, and cross-lane issue index.
- `/events`: lane-grouped evidence signals and event log.
- `/runbook`: chat-led Skill routing reference for Main, Review, Human Review,
  Merge, and Doctor workflows.
- `/settings`: local data-source trust, health, command read matrix, and runtime
  authority settings.

The homepage keeps operations visually secondary and composes focused cockpit
components:

- `WorkflowMap.svelte`: normalized state flow and visible queue pressure.
- `EvidenceColumns.svelte`: lane-grouped event and evidence signals.
- `IntelligenceDashboard.svelte`: tracker, gate, and observability readiness.
- `ReferencePanels.svelte`: Skill handoffs, lane ownership, and evidence writer
  boundaries.
- `DataSourcePanel.svelte`: live, fixture, degraded, or offline data trust.
- `HealthPanel.svelte`: local build, CLI mode, workflow path, and bind status.
- `CommandHealthPanel.svelte`: overview command status, duration, and first
  captured signal.

## Handoff Files

The OpenDesign reference notes and screenshot are preserved in `handoff/`.
