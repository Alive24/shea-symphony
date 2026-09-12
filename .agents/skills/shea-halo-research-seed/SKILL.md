---
name: shea-halo-research-seed
description: Turn a rough agent-loop, harness or observability concern into a neutral evidence-bound Halo Research Issue seed. Use when the user wants to frame, review or start a Halo research item. Do not use for ordinary Issue Forge work, lane execution, or failure diagnosis.
---

# Shea Halo Research Seed

Guide the operator from an uncertain improvement question to an approved Issue
seed. Seed the investigation; do not perform Halo's research or prescribe its
conclusion.

## Establish the contract

Resolve these facts before any tracker write:

- observed repository and checkout;
- target `.shea/halo.toml` or complete local replacement;
- configured GitHub Project, status field, and `Halo Research` state;
- default branch and current remote revision;
- available runtime experiment and verification actions;
- related Issues, snapshots, traces, PRs, and blockers;
- whether the Shea Halo worker can read the Project and target checkout.

Read the target's current configuration and any repository-owned Halo research
issue template named by that configuration. Treat configured workflow actions
and Project status as authoritative. Do not guess a repository, Project,
fixture, revision, or command.

Explain the current control boundary when useful:

- the operator or another automation creates the seed Issue;
- placing its Project item in `Halo Research` starts Halo;
- Halo appends research to that same Issue;
- Halo alone may route verified work to `Todo` or `Done`;
- a human must not pre-authorize the result or force `Todo`.

## Separate context from authority

Use prior Issues heavily as a map, not as a conclusion:

- link the useful workpad decisions, reports, and blockers;
- summarize what the old lifecycle suggested;
- name its exact revision or validity boundary when known;
- mark historical traces and snapshots as context-only unless they are valid
  inputs under the new lifecycle contract;
- require fresh evidence when the base, candidate, configuration, or runtime
  capability has changed.

Never copy raw workpad JSON, traces, prompts, model/tool payloads, credentials,
endpoints, or host paths into a seed.

## Guardrails

- <Safety, locality, privacy, authority, and non-duplication boundaries>

## Create and start safely

Show the complete draft and list assumptions, dependencies, and readiness.
Obtain explicit confirmation before creation. Restate the exact Issue,
repository, Project, and target status before a write.

Use the configured workflow or connected provider to:

1. quality-gate and create the Issue;
2. add it to the configured Project if needed;
3. record structured blockers or relationships;
4. read the Issue and Project item back.

Treat creation and start as separate mutations unless the user explicitly
approves both. Starting means changing the Project item's status to
`Halo Research`; explain that the worker will claim it on its next poll.

After starting, report the Issue URL, number, pinned/freshness assumptions,
Project status, worker prerequisites, and what evidence the operator should
expect next. Do not manually move the item to `Todo`, write Halo's result, reuse
its experimental branch for implementation, or close a predecessor without a
separately confirmed append-only disposition note.

## Interaction style

Keep questions short and conversational. Prefer a recommended phrasing over a
form dump. During drafting, show what remains unresolved. At confirmation,
provide the full seed, readiness classification, exact next mutation, and any
reason not to start yet.

## Deeper references

Read `references/seed-template.md` when drafting or revising the seed body, and
`references/intake-and-readiness.md` when running intake, challenging a draft, or
judging whether a seed is ready to start.
