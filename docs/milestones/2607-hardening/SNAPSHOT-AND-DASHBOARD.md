# Snapshot And Dashboard

Status: Draft

## Purpose

Define what the App reads by default, what Symphony must expose, and where
issue-level detail begins.

The App should be a lightweight operational surface. It should not become a
second tracker, a hidden runtime controller, or an eager artifact reader.

## Dashboard Scope

The top-level dashboard should focus on the current operational surface:

- active lane items;
- human todo items, including `Need to Clarify`, `Need Human Input`, and
  `Human Review`;
- current tracker state;
- PR number and concise PR state when relevant;
- whether autopilot is running, paused, or waiting;
- whether a refresh/cache operation is stale, fresh, or failed.

The dashboard does not need to show historical project queues. Operators can
use the tracker for broad project history and long-tail queue browsing.

The dashboard also does not need worktree path, branch name, trace detail, full
logs, or artifact content. Those belong in lane item detail.

## Lane Item Detail

Issue-level detail may show:

- worktree path;
- branch name;
- PR URL and detailed PR status;
- current workflow step, with future graph node when available;
- recent transition attempts;
- recent error evidence;
- trace id and artifact references;
- workpad and review artifacts;
- doctor output when explicitly opened.

Detail pages may lazy-load artifacts after the operator drills down. Top-level
refresh should not read full transcripts, logs, review reports, or trace
payloads.

## Snapshot Shape

`SymphonySnapshot` should be the first read surface for the App.

It should include:

- snapshot id;
- generated timestamp;
- source timestamps for tracker and local runtime state;
- freshness/staleness markers;
- active issue summaries;
- human todo summaries;
- active PR numbers and concise PR state;
- autopilot state;
- local runtime state needed for display;
- artifact references and short summaries, not full artifact bodies;
- stuck or waiting classification when already represented as workflow state.

It should not include:

- full project history;
- full artifact contents;
- raw transcript bodies;
- full worktree status for every issue;
- source-of-truth state inferred by the App.

## Runtime And Tracker State

Tracker state is the external workflow fact. Runtime state is local execution
evidence.

During lane handoff, Symphony must treat the successful tracker transition as
part of completion. A worker should not be considered fully handed off if the
local runtime believes it is done but the tracker did not move.

When runtime state and tracker state conflict:

- Symphony stops or enters a reconcile path;
- Symphony does not silently guess the next state;
- App displays the conflict as a read-only condition;
- if human attention is needed, Symphony should move the item into
  `Need Human Input` or `Need to Clarify` rather than leaving the issue stuck
  behind UI-only text.

## App Commands

The App may trigger CLI commands that directly support display:

- read a snapshot;
- refresh tracker cache;
- tick or pause autopilot through controlled Symphony commands.

The App should not directly trigger:

- tracker state mutation;
- worktree edits;
- manual worktree opening as a workflow operation;
- agent resume as an operator-side imperative;
- doctor repair flows that modify state outside Symphony policy.

`doctor current issue` may later have two modes:

- automatic doctor checks that Symphony runs itself;
- human doctor work opened through Codex/operator flow when coding-agent help is
  required.

## Stuck State Display

The dashboard should make stuck or waiting state visible through structured
fields, not through hidden interpretation:

- `waiting_on`;
- `blocked_reason`;
- `last_progress_at`;
- `last_transition_attempt`;
- `last_transition_error`;
- `recommended_next_action`.

If the recommended next action requires human attention, the tracker state
should reflect that through `Need Human Input`, `Need to Clarify`, or
`Human Review`.

## MVP Transport

For 2607 hardening, prefer:

- CLI stdout JSON for immediate snapshot reads;
- a local cache file for fast App refresh;
- no daemon requirement in the first pass.

A daemon or local API can be reconsidered after the read/write boundaries and
snapshot shape are stable.

## Performance Bias

Top-level dashboard refresh should be cheap:

- avoid full project history scans;
- avoid full artifact reads;
- avoid full worktree scans;
- avoid mutating commands;
- avoid repeated tracker reads inside one refresh;
- prefer cache plus explicit refresh when needed.

Hard timing targets should come after a measurement pass, but the first design
constraint is clear: local refresh should not feel blocked after LLM work has
already completed.
