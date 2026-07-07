# Subtraction Inventory

Status: Draft

## Purpose

This inventory names the accidental complexity 2607 Hardening should remove or
contain before adding new product capability.

The goal is not to reduce working workflow capability. The goal is to reduce
ownership spread, repeated control-plane work, and unclear state authority.

## Subtraction Areas

All of these are candidates for Phase 1. They do not need a strict priority
order if they can be investigated in parallel.

### Repeated Tracker Reads

Problem:

- Project/tracker state is read repeatedly across commands, lanes, App refresh,
  doctor, and read surfaces.
- The system often feels slow after LLM work has already completed, which points
  at control-plane churn rather than model latency.

Direction:

- Prefer Temporal query-backed workflow snapshots for App reads.
- Use targeted readback after writes instead of repeated full scans.
- Make external waits visible in status snapshots.

### Scattered Tracker Writes

Problem:

- Tracker writes are spread across lane, review, merge, doctor, project, and
  recovery paths.

Direction:

- Route writes through `TrackerTransitionActivity`.
- `IssueWorkflow` requests transitions with evidence; the Activity applies
  them.

Write operations that should use the unified path:

- set status;
- set lane claim field;
- clear lane claim field;
- upsert workpad;
- append timeline evidence;
- link PR;
- set PR/issue relation evidence;
- close issue;
- create follow-up issue.

### Lane-Local State Mapping

Problem:

- Main, Review, Merge, Doctor, and recovery paths can each encode state mapping
  and terminal behavior.

Direction:

- Move standard state vocabulary and transition checks into `IssueWorkflow`.
- Record state transitions in Temporal history and evidence.
- Stop and reconcile when tracker state and runtime state conflict.

### App Refresh And Read Surfaces

Problem:

- App refresh feels slower than it should and may trigger heavy read paths before
  the operator needs artifact-level detail.
- Dashboard state includes both cloud/tracker state and local runtime/memory
  state, so a pure tracker snapshot is not enough.

Direction:

- App dashboard starts from a Temporal query-backed `SymphonySnapshot` that
  includes tracker state and workflow state.
- Artifact files are loaded lazily for drill-down, not for every top-level
  dashboard refresh.
- App refresh must not trigger mutating commands.

### Vendored Runtime And Workspace Layout

Problem:

- Vendored runtime bits in target repos work for MVP dogfood but blur install,
  team config, local config, runtime state, and generated worktree ownership.

Direction:

- Resolve Symphony binary from local install location.
- Keep tracked team config under repo `.shea/`.
- Keep local runtime state and generated worktrees under `~/.shea/` by default.

### CLI Shape Drift

Problem:

- CLI has accumulated product operation semantics that Temporal should own.

Direction:

- Downgrade CLI to admin/dev fallback.
- Move product operations to Temporal workflow start/query/signal/update.
- Remove tick/autopilot/review/merge/doctor-as-mutator command semantics as the
  Temporal migration lands.

### Large Files And Mixed Ownership

Problem:

- Large modules often indicate mixed ownership: runtime control, tracker policy,
  operator evidence, UI read model, and lane behavior in one file.

Direction:

- File moves and module splits are allowed when they clarify ownership and keep
  behavior unchanged.
- Avoid mass movement that is not tied to an ownership boundary.
- Move code behind tests or executable checks so the split is a hardening step,
  not cosmetic churn.

## Top Pain Points

The first two practical pain points are:

1. slowness;
2. states getting stuck without clear cause.

These should guide the first inventory pass.

## First Cut Bias

Prefer a boundary cut before a performance cut:

- identify who owns state transitions;
- identify which Temporal query or Activity result a path used;
- identify whether a component was allowed to write;
- then remove repeated reads and heavy refresh paths.

Performance fixes that do not clarify ownership risk becoming local patches.

## Deferred During Phase 1

Defer broad work on:

- new user-visible UI features;
- new tracker adapters;
- new LLM reviewer types;
- new Issue Forge capability;
- a real plugin runtime;
- a full Workflow Graph editor.

File moves are not forbidden. Broad file movement without a clear ownership
boundary is deferred.
