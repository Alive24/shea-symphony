# Operator Action Bridge

Status: Draft

## Purpose

Define how routed Coding Agent/operator flows submit human input, approval,
human fixes, rework requests, and doctor handoff results back into Symphony.

Prompt text is not a permission boundary. Coding Agents should receive a
short-lived capability context and a narrow tool/MCP submit bridge, not raw
Temporal or tracker access.

## Principle

```text
Prompt explains the task.
Context defines what is allowed.
Tool/MCP bridge is the only action channel.
Temporal Update validates the action.
Activities commit tracker/read-model changes.
```

The bridge is intentionally narrow. It does not write tracker state, edit
worktrees, update SQLite directly, run merge, or call arbitrary workflow
operations.

## Flow

```text
App human todo item
  -> Tauri backend prepare_operator_action(workflow_id, action_kind)
  -> Symphony validates current Workflow state
  -> creates OperatorActionContext
  -> App opens Codex/operator flow with context path and brief
  -> Coding Agent/operator performs the work
  -> Coding Agent/operator calls submit_operator_action tool/MCP
  -> bridge validates context, action, expiry, and payload schema
  -> bridge sends Temporal Update
  -> Workflow validates state and action again
  -> Activities update tracker/read model
```

Use a tool/MCP bridge as the primary submit path. Do not require the Coding
Agent to shell out through a CLI for normal operator actions. CLI wrappers, if
any, are debug/admin fallback only.

## OperatorActionContext

Context should be stored under local runtime state, not tracked repo config.

Recommended location:

```text
~/.shea/operator-actions/<context-id>/
```

Recommended files:

```text
context.json
brief.md
```

Recommended context shape:

```text
OperatorActionContext {
  context_id
  workflow_id
  issue_ref
  current_state
  allowed_actions
  requested_action
  artifact_refs
  created_at
  expires_at
  capability_ref
}
```

`capability_ref` is an opaque local capability reference. It is not a tracker
credential and should not grant broad Temporal access.

## Allowed Actions

Use enum allowlists, not free-form command names.

Initial actions:

- `submit_human_input`;
- `approve_human_review`;
- `request_rework`;
- `submit_human_fix`;
- `doctor_handoff_result`.

The bridge must reject actions that are not present in the context's
`allowed_actions`.

## Tool Interface

Primary interface:

```text
submit_operator_action(context_id, action, payload)
```

Optional helper tools:

```text
read_operator_action_context(context_id)
list_operator_action_artifacts(context_id)
```

Do not expose a broad Temporal client, tracker client, SQLite writer, or
workflow mutation API as part of this tool surface.

## Submission Payload

Recommended payload:

```text
OperatorActionSubmission {
  context_id
  action
  actor
  summary
  evidence_refs
  result_payload
  created_at
}
```

Action-specific requirements:

- `approve_human_review`: approval summary and evidence refs;
- `request_rework`: requested changes, rationale, and evidence refs;
- `submit_human_fix`: diff/PR/artifact summary and evidence refs;
- `submit_human_input`: answer summary and evidence refs;
- `doctor_handoff_result`: diagnosis summary, recommended next action, and
  evidence refs.

Submissions without evidence should be rejected unless the action schema
explicitly marks evidence as optional.

## Validation

The bridge performs local validation:

- context exists;
- context is not expired;
- action is allowed;
- payload matches action schema;
- required evidence refs exist or are intentionally marked unavailable.

The Workflow performs authoritative validation again:

- workflow is still in the expected state;
- requested action is still valid for that state;
- context has not expired;
- payload schema and evidence satisfy the state policy;
- duplicate submission is idempotent or rejected with a typed result.

## Temporal Update

Use Temporal Update for actions that need synchronous accepted/rejected
feedback, including:

- `approve_human_review`;
- `request_rework`;
- `submit_human_fix`;
- `doctor_handoff_result` when it changes workflow direction.

Signal may be used for low-risk fire-and-continue notes or supplemental
artifact refs, but state-changing operator actions should prefer Update.

## Observability

Temporal history records the submitted action and validation outcome.

SQLite may project current or recent status through `activity_progress`,
`workflow_index`, or artifact refs. SQLite is not the action ledger.

## Non-Goals

- No raw tracker mutation access.
- No raw Temporal client access for Coding Agent flows.
- No CLI requirement for normal action submission.
- No broad MCP server exposing arbitrary Symphony operations.
- No App UI implementation of human review or rework policy.

