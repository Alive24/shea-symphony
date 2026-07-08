# Runtime Role Mapping

Status: Draft

## Purpose

Translate the abstract Temporal architecture direction into Shea Symphony's
current repo shape.

The guiding split is:

```text
Temporal orchestrates.
Codex implements.
Review backends review.
Shea evaluates semantics.
Symphony commits state.
```

This document is not a new framework proposal. It is a grounding document for
2607 implementation boundaries.

## Repo Reality

The current repo already has useful seams:

- `src/codex_app_server.rs` normalizes Codex app-server events;
- `src/lanes/main_loop.rs` owns the current Main lane loop;
- `src/lanes/review/automatic.rs` owns automatic review lane behavior;
- `src/merge_lane.rs` owns merge lane decisions and merge repair evidence;
- `src/tracker/*` owns tracker adapters and tracker-specific evidence;
- `src/doctor/*` owns doctor reports;
- `app/src/*` owns operator presentation.

2607 should migrate these behaviors into Temporal-backed boundaries without
renaming every existing concept first.

## Temporal

Temporal is the durable local control plane.

It owns:

- workflow lifecycle;
- durable issue state;
- waits, timers, retries, cancellation, and timeout policy;
- signals, updates, and queries;
- activity scheduling;
- workflow history and activity attempt evidence.

Temporal does not own:

- coding-agent reasoning;
- prompt authoring;
- tracker-specific low-level API details;
- UI presentation;
- large logs or transcripts.

## Codex App-Server

Codex app-server is the default coding task runtime, not merely a low-level
tool. Symphony should let it do its best and evolve with OpenAI instead of
rebuilding Codex's internal agent/tool loop inside Temporal.

In 2607, the intended boundary is coarse:

```text
RunCodexImplementationActivity(input) -> CodexImplementationResult
```

The Activity may internally use the existing app-server protocol, session
registry, worktree setup, prompts, and skills. Temporal should not model every
Codex tool call or model turn as a separate Activity.

The same rule applies to future coding agents: Symphony owns the durable
attempt boundary; the coding agent owns its internal execution loop.

Inputs should be structured and small:

- issue id;
- issue contract;
- repo/worktree reference;
- branch or worktree policy;
- task instruction;
- allowed scope;
- repair or review feedback, when relevant;
- artifact root;
- timeout or budget.

Outputs should be summaries and references:

- Codex session/thread id;
- changed file summary;
- branch or PR reference;
- test summary;
- result status;
- human-input request, if any;
- artifact refs for logs, transcripts, patches, and reports.

Large Codex transcripts belong in local artifacts, not Temporal history.

## Review Backend

The existing automatic review lane already has review backend concepts, with
Gemini as a current backend.

2607 should model review as:

```text
AgentReviewActivity(input) -> AgentReviewResult
```

Gemini can remain the configured backend, but the core Activity name should not
hard-code Gemini. The Activity returns a typed verdict:

- `accept`;
- `reject`;
- `needs_human_input`;
- `error`.

The verdict should include:

- blocking issues;
- required changes;
- evidence gaps;
- risk summary;
- confidence;
- artifact refs.

Temporal decides the next workflow state. The review model does not directly
write tracker state or move the issue.

## Shea

Shea is product semantics over the runtime.

It owns:

- issue contracts;
- evidence schemas;
- lane policies;
- skills;
- prompt templates;
- semantic gates;
- operator-facing copy and interaction policy;
- backlog shaping through Issue Forge, Dream, and Reflect style flows.

Shea may call LLMs, but LLM results must become structured proposals,
verdicts, evidence, or questions before Symphony commits state.

## Naming Boundary

Use `symphony` as the core runtime package/module boundary. Temporal is the
runtime spine and default context inside Symphony; a separate
`temporal_runtime` name is unnecessary unless a concrete implementation
constraint appears.

Shea extension code may also use Temporal-backed Activities, Queries, Signals,
and Updates. The difference is ownership, not whether Temporal is involved:
Symphony owns durable workflow control and state commits; Shea owns semantic
extensions and operator-facing policy.

## Coarse Activity Boundary

2607 should prefer durable coarse Activities over turn-level orchestration.

Good initial Activity boundaries:

- `ContractCheckActivity`;
- `RunCodexImplementationActivity`;
- `AgentReviewActivity`;
- `HumanReviewValidationActivity`;
- `ReworkActivity`, or `RunCodexImplementationActivity` with rework context;
- `MergeActivity`;
- `MergeSemanticFixActivity`, only if not folded into `MergeActivity`;
- `DoctorActivity`;
- `TrackerTransitionActivity`;
- `ArtifactWriteActivity`.

Avoid splitting into per-tool-call Activities unless the existing code already
has that boundary or a measured retry/cancellation need forces it.

## Idempotency

Every side-effecting Activity must accept stable ids or idempotency keys.

Recommended keys:

- workflow id;
- issue id;
- lane name;
- attempt number;
- activity purpose;
- branch name;
- PR id;
- artifact id.

Activity retries must not create duplicate tracker comments, duplicate claims,
duplicate worktrees, or duplicate PR operations.

## Deferred Frameworks

Do not add Rig in 2607.

Rig may be revisited later only if Shea needs a Rust-native external AI layer
for structured judging, multi-provider eval, MCP tools outside Codex, or
independent evaluator agents that Codex app-server and Skills do not cover.

Do not add a broad MCP server in 2607.

MCP may later expose selected Temporal/Symphony operations to external agents,
but it should be an interface over the Temporal runtime, not the internal
orchestration model.

One narrow exception is allowed in 2607: the Operator Action Bridge may be
implemented as a tool/MCP surface for routed Codex/operator flows. That bridge
only submits scoped operator action results from an `OperatorActionContext`; it
does not expose raw Temporal, tracker, SQLite, or workflow mutation APIs.

Do not build vector RAG in 2607.

Start with deterministic context:

- tracker issue contract;
- workpad/evidence;
- git diff;
- file map;
- tests;
- repo metadata;
- explicit artifacts.

Temporal-managed context ingestion and vector search can be evaluated after
the durable runtime boundary is working.

## Implementation Bias

Preserve working abstractions when possible:

- move lane loops behind Activities before deleting behavior;
- wrap existing tracker adapter calls before replacing adapters;
- reuse existing Codex app-server event normalization;
- reuse current review backend decisions before generalizing providers;
- keep App read models presentation-focused while replacing their data source
  with Temporal queries.

The goal is to delete the duplicate orchestration loop, not to rewrite every
module under a new vocabulary.
