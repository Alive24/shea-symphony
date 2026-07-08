# Symphony And Shea Boundaries

Status: Draft

## Symphony

Symphony is the reliable local Temporal workflow runtime.

Symphony owns:

- workflow structure and future graph compatibility;
- standard tracker states;
- local Temporal workflow and worker ownership;
- tracker reads and writes;
- GitHub Project v2 adapter;
- future tracker adapter interface for Linear and other trackers;
- canonical worktree discovery;
- issue worktree lifecycle;
- agent runner lifecycle;
- review stage execution;
- merge stage execution;
- Temporal workflow state;
- retry, stall, and reconciliation;
- event log and traceability;
- status snapshots;
- extension node execution policy.

Symphony should be deterministic where correctness, safety, or resumability
depends on it. LLMs should not decide raw tracker mutation, worktree ownership,
claim ownership, or terminal cleanup.

## Shea

Shea is an extension layer over Symphony.

Shea owns:

- skills;
- prompt templates;
- semantic gates;
- Issue Forge;
- Dream and Reflect style backlog mining;
- operator interaction policy;
- project-specific workflow extensions;
- App-specific presentation of Symphony snapshots.

Shea may use LLMs, but LLM output must be converted into structured evidence,
questions, or transition proposals before Symphony acts on it.

## Hard Rules

- Symphony writes tracker state through `TrackerTransitionActivity`.
- Shea and extension nodes do not write tracker state directly.
- Tracker writes must go through `TrackerTransitionActivity`.
- Workflow participants may propose transitions, but `IssueWorkflow` decides
  and `TrackerTransitionActivity` commits them.
- State transitions must be recorded in Temporal history and evidence.
- Standard nodes are not replaced in place. They may be disabled or bypassed by
  graph configuration.
- App surfaces consume Symphony snapshots. They do not become source-of-truth
  state.
- Workspaces created by Symphony do not live inside the canonical worktree by
  default.

If runtime state and tracker state conflict, tracker state is the external
fact and runtime state is local execution evidence. Symphony should stop to
reconcile rather than guessing.

## State Ownership

Standard tracker states are:

- `Backlog`
- `Todo`
- `Need to Clarify`
- `In Progress`
- `Need Human Input`
- `Agent Review`
- `Human Review`
- `Merging`
- `Rework`
- `Done`

Workflow Graph configuration may disable or bypass optional stages, including
`Agent Review`, but the state remains part of the standard vocabulary.

## LLM Boundary

LLM nodes may inspect context, summarize, critique, ask questions, or propose a
transition. They do not directly mutate tracker state or file systems unless an
explicit node policy allows workspace writes.

Extension nodes may influence graph direction by recommending the next edge or
core node. The boundary is commit authority, not proposal authority.

## UI Boundary

The App may:

- display current graph and node state;
- display status, logs, and traceability;
- initialize and operate local Temporal-backed workflows through Tauri backend
  commands.
- route human todo actions to Codex/operator flows.

The App should not:

- directly edit tracker state;
- directly modify worktrees;
- bypass Temporal workflow or Activity boundaries;
- implement human input, approval, rework, or human doctor semantics directly
  in UI code.
