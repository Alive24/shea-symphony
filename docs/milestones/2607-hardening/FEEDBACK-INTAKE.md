# Feedback Intake

Status: Draft

## Purpose

Define how to preserve hackathon and dogfood feedback without turning every
old issue into immediate 2607 implementation work.

The feedback is real operating evidence. The mistake would be treating raw
feedback as an executable issue before it has been shaped into a contract.

## Feedback Seed

A feedback seed is an issue, comment, note, or operator report that captures a
real problem or friction point but is not ready to execute.

Examples:

- App refresh felt slow after the LLM had already finished.
- PR failed to link to its issue.
- Human Review was skipped or unclear.
- Agent Review was missing.
- Workflow state got stuck without clear reason.
- A hackathon task required repeated human doctor intervention.
- Forge-created executable issues carried Project fields but missed GitHub
  milestone/labels because the protected creation path did not yet expose those
  repo metadata writes.
- Forge-created executable issues could be claimed by autoloop before final
  create readback, making the command report failure even though the issue was
  successfully created and legally advanced.

Feedback seeds may remain open if they are useful evidence. They should not
sit in active executable lanes unless they have been shaped.

## Default Handling

For old hackathon feedback issues:

1. Add `feedback:hackathon`.
2. Add `needs:shaping` unless already shaped.
3. Set Project `Package = Feedback Intake`.
4. Set Project `Slice = feedback`.
5. Keep or move the issue to `Backlog`.
6. Add a short issue comment or timeline note explaining what was captured and
   how it may be promoted later.
7. Do not attach `2607-Hardening` milestone until it becomes executable.

This keeps the feedback visible without forcing immediate implementation.

## Absorption Paths

A feedback seed can move through one of these paths.

### Absorb Into 2607

Use when the feedback is directly covered by the hardening plan.

Example:

- PR not linking reliably becomes evidence for
  `T2607-04 TrackerTransitionActivity`.
- App refresh slowness becomes evidence for
  `T2607-02 Local State DB`, `T2607-07 App Integration`, or
  `T2607-08 Deletion And Performance`.
- Forge metadata gaps become evidence for moving milestone/label writes into a
  durable, readback-verified tracker mutation boundary instead of normalizing
  raw post-create GitHub edits.
- Forge create/readback races become evidence for durable write receipts and
  readback reconciliation that can classify already-advanced tracker state
  without asking operators to retry creation.

Handling:

- link the executable 2607 issue;
- add a short comment naming the package and successor issue;
- close the feedback issue only after the evidence is captured somewhere
  durable.

### Defer To 2608+

Use when the feedback points to Workflow Graph, extension modules, visual graph
editing, or later product capability.

Handling:

- set `Package = 2608 Workflow Graph` or `Future`;
- leave it in Backlog or close after linking to a future planning artifact.

### Superseded By Docs

Use when the feedback is answered by a new architecture decision or package
doc, but no direct implementation issue is needed.

Handling:

- comment with the canonical doc link;
- close if no further tracking is useful.

### Close After Capture

Use when the feedback was useful historically but is too broad, duplicated, or
already covered by multiple successor issues.

Handling:

- summarize what was captured;
- link successor issues/docs;
- close with no milestone.

## Shaping Into Executable Issues

Before a feedback seed becomes a 2607 executable issue, it needs:

- a concrete outcome;
- owner package;
- slice type;
- acceptance checks;
- dependencies;
- evidence refs;
- explicit non-goals;
- confirmation that it can run through one normal Shea Symphony workflow pulse.

Once shaped:

- set the relevant `T2607-xx` Package;
- set `Slice`;
- add `2607-Hardening` milestone;
- add area/kind/risk labels;
- move to `Todo` only when ready.

## Do Not Do

Do not:

- bulk-move old feedback into `Todo`;
- attach all old feedback to `2607-Hardening`;
- close old feedback without preserving what was learned;
- create giant umbrella issues from feedback piles;
- use feedback seeds as acceptance criteria unless the successor issue names
  the exact behavior to validate.

## Review Cadence

During 2607, review feedback seeds at package boundaries:

- before creating new T2607 executable issues;
- after completing each major package;
- before closing 2607 hardening.

The goal is not to empty feedback intake immediately. The goal is to ensure
real dogfood evidence is either absorbed, deferred, superseded, or explicitly
closed after capture.
