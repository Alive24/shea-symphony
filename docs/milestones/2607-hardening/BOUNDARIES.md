# Symphony And Shea Boundaries

Status: Draft

## Symphony

Symphony is the reliable workflow runtime.

Symphony owns:

- workflow graph loading and validation;
- standard tracker states;
- tracker reads and writes;
- GitHub Project v2 adapter;
- future tracker adapter interface for Linear and other trackers;
- canonical worktree discovery;
- issue worktree lifecycle;
- agent runner lifecycle;
- review stage execution;
- merge stage execution;
- runtime state;
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
- App-specific interpretation of Symphony snapshots.

Shea may use LLMs, but LLM output must be converted into structured evidence,
questions, or transition proposals before Symphony acts on it.

## Hard Rules

- Symphony writes tracker state.
- Shea and extension nodes do not write tracker state directly.
- Tracker writes must go through one Symphony-owned transition command path.
- Workflow participants may propose transitions, but Symphony decides and
  commits them.
- State transitions must be recorded as runtime events.
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
- control tick/autopilot execution.

The App should not:

- directly edit tracker state;
- directly modify worktrees;
- bypass Symphony transition checks.
