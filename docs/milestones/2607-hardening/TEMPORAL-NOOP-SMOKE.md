# Local Temporal No-Op Smoke

Status: T2607-01 development and integration-test harness

## Supported Command

Run exactly one repo-owned local startup and smoke path:

```sh
./scripts/temporal-noop-smoke
```

Prerequisite: the official `temporal` CLI must be on `PATH` and support
`temporal server start-dev`. The script is the explicit opt-in: it sets
`SHEA_TEMPORAL_SMOKE=1` before invoking the ignored integration test. When the
harness needs to start a test-owned service, it prints `temporal --version`;
retain that line with the command output as version evidence.
No Temporal Cloud account, GitHub credential, tracker, agent launch, worktree,
SQLite projection, or Shea artifact root is used.

The Temporal upstream repository continues to document `temporal server
start-dev` as its local development command, and the official setup action
uses the headless form suitable for CI and test harnesses. The harness uses:

```text
temporal --disable-config-env --disable-config-file server start-dev --headless --ip 127.0.0.1 --port 7233
```

This is harness implementation evidence, not a second command for developers
to invoke directly.

It deliberately omits `--db-filename`, so the service uses its non-persistent
development storage. This is a local runtime proof, not a production deployment
or a source of local SQLite state. The two `--disable-config-*` flags keep an
operator's Temporal CLI configuration or environment from changing the
repo-owned smoke startup path.

## Ownership And Lifecycle

The checked-in `.shea/workflows/shea-symphony.md` profile is the only profile
used by both the test client and the test-owned worker. `src/main.rs` now
defaults to that same path; an explicit `SHEA_WORKFLOW_PATH` is passed to the
worker child so an operator's legacy shell setting cannot select
`workflows/shea-symphony.md`.

The command follows this bounded sequence:

1. Probe `localhost:7233` through `SymphonyTemporalClient::check_service`.
2. If it is reachable, stop with the `existing Temporal service` diagnostic.
   The harness does not share queues with, inspect, or terminate that service.
3. If it is unavailable, start the exact headless dev-server command above and
   retry readiness at most 40 times, 250 ms apart.
4. Start one test-owned Symphony worker process using the checked-in profile.
   Its normal `symphony-core`, `symphony-agent`, and `symphony-local` worker
   registrations are used; no raw client, test queue, or second scheduler is
   introduced.
5. Start one unique synthetic `IssueWorkflow`, observe its read-only query
   while a test-owned Activity-only hold keeps it open, then observe its
   terminal `NoopCoreActivity` result.
6. Stop and reap the exact worker and dev-service child processes created by
   the harness on success, timeout, assertion failure, or test unwinding.

The query hold is only honored by the test worker for a synthetic issue prefix;
normal product inputs never enter that branch. It is Activity code, never
Workflow code, so the Workflow keeps its replay-deterministic behavior.

## Expected Successful Output

The workflow ID is intentionally unique, so its exact value changes. A success
has this shape:

```text
temporal smoke: Temporal CLI <installed-version>
temporal smoke: test-owned dev service is ready at localhost:7233
temporal smoke: observed read-only query for smoke:shea-symphony:<pid>:<time>:<sequence>
temporal smoke: observed completed NoopCoreActivity result for smoke:shea-symphony:<pid>:<time>:<sequence>
temporal smoke: cleaned up test-owned worker and dev service
```

The terminal assertion requires `active_step=noop_completed`,
`terminal_outcome=completed_noop`, the existing no-side-effect
`NoopCoreActivity` summary, and no artifact references. Its input uses only
`synthetic/temporal-smoke` and `synthetic:temporal-smoke:*`; it never targets a
real tracker issue.

## Diagnostics

| Diagnostic stage | Meaning | Operator action |
| --- | --- | --- |
| `dev-server-prerequisite` | `temporal` is missing or cannot report a version. | Install or expose the official Temporal CLI, then rerun the command. |
| `existing Temporal service` | Something reachable already owns `localhost:7233`. | Leave it untouched; stop it only if appropriate, then use a clean local environment. |
| `service-probe` / `service-readiness-timeout` | The service endpoint is uncertain or the test-owned dev server did not become reachable. | Check the bounded child-output tail and port availability; do not run a second server manually. |
| `worker-registration` | The test-owned worker exited before it could service the smoke. | Check its bounded child-output tail for profile, SDK registration, or connection details. |
| `workflow-query-timeout` | The Workflow did not become queryable within the bounded worker-ready window. | Check worker registration and local service health. |
| `terminal-result-timeout` / `terminal-result-failure` | The Workflow did not complete with the expected no-op result. | Inspect only the reported bounded diagnostic; this is not a tracker or agent failure. |

`SymphonyTemporalClient::check_service` also makes the unavailable-service path
typed and actionable without launching a dev server. The ordinary unit test
uses `127.0.0.1:1` to cover that behavior.

## Compatibility Evidence

`Cargo.lock` resolves the pinned Temporal Rust SDK family to 0.5.0
(`temporalio-client`, `temporalio-common`, `temporalio-macros`,
`temporalio-sdk`, and `temporalio-sdk-core`). The harness stays inside the
existing `symphony` client and worker boundaries so future SDK upgrades remain
one compatibility surface.

The live reference run on 2026-07-19 used the official macOS arm64 Temporal
CLI `1.8.0` (`Server 1.31.2`, `UI 2.50.1`) and completed the command above
with a test-owned service. The command prints its installed version on every
fresh-service run so later evidence records the exact CLI/server pair instead
of assuming this reference version.

The dev-server command was reconfirmed against the official Temporal source
and CLI setup guidance on 2026-07-19:

- https://github.com/temporalio/temporal#download-and-start-temporal-server-locally
- https://github.com/temporalio/setup-temporal#usage

This smoke establishes only the runtime spine. Coordinator policy, tracker
transitions, SQLite projections, agent Activities, App controls, and workflow
graph behavior remain deferred to their dedicated T2607 slices.
