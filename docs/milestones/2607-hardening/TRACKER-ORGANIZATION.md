# Tracker Organization

Status: Draft

## Purpose

Define how 2607 Hardening work should be organized in GitHub Issues,
Milestones, and GitHub Project v2.

The goal is to keep issues executable while preserving the 2607 architecture
structure from the milestone docs.

## Core Decision

Target GitHub Project:

```text
https://github.com/users/Alive24/projects/9
```

Use one GitHub Milestone:

```text
2607-Hardening
```

Do not create one GitHub Milestone per implementation package. GitHub
Milestones represent release/milestone completion scope. Implementation
packages are architecture/workstream grouping and should live in Project
fields.

## GitHub Project Fields

Keep the existing `Status` field as the workflow lane state.

Keep the existing `Capability` field as the product/system capability area.
`Package` is different: it identifies the 2607 implementation package that owns
the executable issue.

Add these Project fields:

### `Package`

Single-select field for the 2607 implementation package or feedback bucket.

Recommended options:

- `T2607-01 Temporal Runtime Skeleton`
- `T2607-02 Local State DB`
- `T2607-03 Workflow Coordinator`
- `T2607-04 TrackerTransitionActivity`
- `T2607-05 Agent Activity Boundary`
- `T2607-06 IssueWorkflow State Machine`
- `T2607-07 App Integration`
- `T2607-08 Deletion And Performance`
- `Feedback Intake`
- `2608 Workflow Graph`
- `Future`

Use `Package` for grouping views and understanding ownership. Do not use it as
workflow state.

### `Slice`

Single-select field for the implementation slice type.

Recommended options:

- `contract`
- `dto`
- `skeleton`
- `adapter`
- `activity`
- `workflow`
- `query`
- `projection`
- `migration`
- `deletion`
- `test`
- `instrumentation`
- `docs`
- `feedback`

Use `Slice` to keep issue size honest. A package may have many executable
issues with different slices.

## Labels

Use labels for cross-cutting filters, not primary structure.

Recommended labels:

- `2607-hardening`
- `feedback:hackathon`
- `needs:shaping`
- `area:temporal`
- `area:sqlite`
- `area:tracker`
- `area:agent`
- `area:app`
- `area:workflow`
- `kind:contract`
- `kind:migration`
- `kind:deletion`
- `kind:test`
- `kind:instrumentation`
- `risk:tracker-write`
- `risk:runtime-spine`
- `risk:agent-side-effect`

Avoid creating one label for every implementation package if the Project
`Package` field already exists.

## Views

Recommended Project views:

- `2607 Board`: existing workflow `Status` board for active executable issues.
- `By Package`: group by `Package`.
- `Feedback Intake`: filter `Package = Feedback Intake`, `Slice = feedback`,
  or `feedback:hackathon`.
- `Deletion Queue`: filter `Slice = deletion` or `kind:deletion`.
- `Risky Writes`: filter `risk:tracker-write`.
- `App/Perf`: filter `area:app` or `kind:instrumentation`.
- `Blocked/NHI`: existing human-input and review states.

View configuration can be adjusted in GitHub UI if the Project API cannot
express the desired grouping/filtering cleanly.

## Executable Issue Contract

Every executable 2607 issue should:

- belong to the `2607-Hardening` milestone;
- have `Package` set to one `T2607-xx` package;
- have `Slice` set;
- include acceptance checks copied or narrowed from the package docs;
- fit inside one normal Shea Symphony workflow pulse;
- be reviewable independently;
- name dependencies and parallel-safe issues;
- avoid reintroducing direct tracker writes, App-owned workflow policy, or the
  old autopilot loop.

## Not Recommended

Do not create umbrella implementation issues such as:

```text
Implement T2607-04 TrackerTransitionActivity
```

That shape hides progress and creates large, ambiguous work.

Prefer executable issues such as:

```text
T2607-04: define TrackerTransitionRequest/Result DTOs
T2607-04: migrate PR-to-issue link into durable mutation activity
T2607-04: block direct tracker writes from App/CLI/lane paths
T2607-04: add readback/idempotency tests for tracker transitions
```

## Promotion Flow

Use this flow when turning docs into GitHub issues:

```text
implementation package doc
  -> choose a small slice
  -> draft executable issue contract
  -> assign Package/Slice
  -> attach 2607-Hardening milestone
  -> add labels for area/kind/risk
  -> move to Todo only when executable
```

If the issue is not executable, keep it in `Backlog`, `Need to Clarify`, or
`Feedback Intake` rather than moving it into the active 2607 queue.

### Dogfood Finding: Forge Metadata Coverage

During creation of #475 through the protected 2606 MVP `forge create` path,
Forge correctly owned the issue body, assignee, Project insertion, `Status`,
`Package`, and `Slice`, but it did not expose repo metadata writes for GitHub
milestone or labels.

Do not patch this gap with ad hoc post-create `gh issue edit` calls in the
normal workflow. The desired 2607 direction is to bring milestone and label
writes into the same durable, readback-verified tracker mutation boundary as
other issue creation metadata. Until that exists, newly forged executable
issues may have correct Project fields while missing repo milestone/labels;
record that as dogfood evidence rather than weakening the Forge ownership
model.

## Feedback Items

Hackathon feedback and dogfood findings are valuable, but they are not
automatically executable implementation work.

Feedback items should either:

- become executable 2607 issues after shaping;
- be absorbed into an existing 2607 issue as evidence;
- be deferred to `2608 Workflow Graph` or `Future`;
- be closed after the feedback is captured in canonical docs or a successor
  issue.

Do not attach `2607-Hardening` milestone to feedback seeds until they are
shaped into executable issues.

Feedback seeds do not need a separate Project field beyond `Package` and
`Slice`. Use `Status = Backlog`, `Package = Feedback Intake`, `Slice =
feedback`, labels, and an issue comment/timeline log. Promotion changes those
fields into the executable package and slice.
