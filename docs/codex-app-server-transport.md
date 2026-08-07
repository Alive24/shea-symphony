# Codex App-Server Transport

Shea Symphony supports a minimal local Codex app-server transport for Codex-backed agent runs. This transport is a stdio JSON-RPC harness, not a persistent daemon, remote worker, or full Symphony dynamic-tool implementation.

## Supported Protocol Subset

The harness launches the configured `codex.command` in the prepared issue workspace and sends these messages:

- `initialize` with experimental API capability metadata.
- `initialized` notification after the initialize response.
- `thread/start` with approval policy, thread sandbox, workspace `cwd`, and an empty `dynamicTools` list.
- `thread/resume` with the recorded thread id, approval policy, thread sandbox, and workspace `cwd` before continuation turns for recovered app-server sessions.
- `turn/start` with text input, workspace `cwd`, issue title when available, approval policy, and configured `turn_sandbox_policy` when present.
  Codex-backed Review turns also attach the backend-neutral Review output schema.

The harness reads line-delimited JSON-RPC output and normalizes:

- `turn/completed` into `AgentEvent::Completed`.
- `turn/failed` and `turn/cancelled` into `AgentEvent::Failed`.
- `thread/tokenUsage/updated` into `AgentEvent::TokenUsage` when token fields are available.
- notifications and methodless messages into `AgentEvent::Message`.
- malformed protocol output into a fail-closed `AgentEvent::Failed`.
- input-required, approval-required, and unsupported tool-call events such as `turn/input_required`, `item/tool/requestUserInput`, `tool/requestUserInput`, `item/commandExecution/requestApproval`, `execCommandApproval`, `applyPatchApproval`, `item/fileChange/requestApproval`, and `item/tool/call` into fail-closed `AgentEvent::Failed`.

Partial stdout lines are buffered by the line reader until a newline or EOF before normalization. Unsupported request-for-input, approval-required, or tool-call behavior must fail clearly rather than leaving an unattended run waiting for an operator.

## Artifacts

Each app-server run records:

- the rendered prompt artifact;
- raw stdin/stdout protocol JSONL;
- stderr log;
- normalized event JSON;
- final process exit status when available.

After a turn starts, the transport enforces `codex.stall_timeout_ms` as a
protocol-event inactivity timeout. This is separate from the full
`codex.turn_timeout_ms`: long-running turns may continue as long as events keep
arriving, while silent turns are killed and recorded as backend failures.

The final `AgentEvent::Message` contains `prompt_artifact=`, `protocol_artifact=`, `stderr_artifact=`, `normalized_events_artifact=`, and `exit_status=` fields so status, runtime-state, Doctor, and workpad surfaces can point to durable evidence.

## Independent Review Adapter

`review_lane.backend: codex-app-server` reuses this transport behind the existing
Review scheduler, claim, worker, artifact, ledger, report, and routing
boundaries. A new Review job always sends `thread/start`; it never imports a
Main or Merge thread. If that job's app-server process is interrupted after its
thread identity is recorded, the same in-memory job may make one
`thread/resume` attempt for that exact identity. A later job starts fresh.

The Review adapter requires `codex_approval_policy: never` and a read-only
thread sandbox. It validates the structured terminal classification and finding
schema, including severity, file, line, and evidence, before constructing the
backend-neutral `AgentReviewReport`. Process exit alone is never a pass.
Approval, input, unknown server requests, malformed or truncated protocol,
missing structured output, timeout, cancellation, unexpected exit, or any
workspace change produces a non-pass backend result with preserved protocol
evidence.

```yaml
codex:
  command: codex app-server -c 'service_tier="fast"'
review_lane:
  backend: codex-app-server
  # codex_command: /absolute/path/to/codex app-server
  codex_approval_policy: never
  codex_thread_sandbox: read-only
  codex_turn_sandbox_policy:
    type: readOnly
```

## Non-Goals

This transport does not:

- choose lane defaults by itself; Main and merge-agent defaults are owned by
  workflow config and lane wiring;
- change Gemini or agy Review behavior;
- implement remote SSH app-server launch;
- implement dynamic tool execution;
- keep a long-lived daemon between turns;
- replace the CLI-owned clean merge path.

Those behaviors are owned by later app-server wiring and runtime-status issues in the parent batch.
