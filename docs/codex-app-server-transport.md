# Codex App-Server Transport

Jade Symphony supports a minimal local Codex app-server transport for Codex-backed agent runs. This transport is a stdio JSON-RPC harness, not a persistent daemon, remote worker, or full Symphony dynamic-tool implementation.

## Supported Protocol Subset

The harness launches the configured `codex.command` in the prepared issue workspace and sends these messages:

- `initialize` with experimental API capability metadata.
- `initialized` notification after the initialize response.
- `thread/start` with approval policy, thread sandbox, workspace `cwd`, and an empty `dynamicTools` list.
- `turn/start` with text input, workspace `cwd`, issue title when available, approval policy, and configured `turn_sandbox_policy` when present.

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

The final `AgentEvent::Message` contains `prompt_artifact=`, `protocol_artifact=`, `stderr_artifact=`, `normalized_events_artifact=`, and `exit_status=` fields so status, runtime-state, Doctor, and workpad surfaces can point to durable evidence.

## Non-Goals

This transport does not:

- choose lane defaults by itself; Main and merge-agent defaults are owned by
  workflow config and lane wiring;
- change Gemini Review behavior;
- implement remote SSH app-server launch;
- implement dynamic tool execution;
- keep a long-lived daemon between turns;
- replace the CLI-owned clean merge path.

Those behaviors are owned by later app-server wiring and runtime-status issues in the parent batch.
