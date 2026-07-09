---
name: shea-symphony-issue-forge
description: Use when creating, shaping, or validating Shea Symphony GitHub issues from rough operator intent. Runs a conversation-first discuss flow, resolves gate-critical ambiguity, drafts a quality-gated issue, asks for explicit confirmation, then creates it through Shea Symphony forge create.
metadata:
  short-description: Conversational Shea Symphony issue forge
  suite-version: 2026.05.22
---

# Shea Symphony Issue Forge

Create Shea Symphony issues through a conversation-first workflow. Do not jump
straight to `forge create` from rough intent unless the user explicitly provides
a complete issue body.

## Runtime Topology

Live Shea Symphony issue creation still runs through the protected 2606 MVP
runtime until 2607 replaces the runtime spine. Use the MVP worktree for
CLI/App execution and the active Shea Symphony development worktree only as
source context.

MVP runtime worktree:

```bash
/Users/chuntengxiao/.shea-symphony/mvp/shea-symphony-2606-mvp
```

Canonical workflow inside the MVP runtime:

```bash
workflows/shea-symphony.md
```

Run deterministic tracker and forge mutations from the MVP runtime:

```bash
cd /Users/chuntengxiao/.shea-symphony/mvp/shea-symphony-2606-mvp
cargo run -- forge validate --workflow workflows/shea-symphony.md --issue '#<issue>'
cargo run -- forge create --workflow workflows/shea-symphony.md ...
```

If the Tauri App is needed, start it from the MVP runtime `app/` directory with
the local MVP profile:

```bash
cd /Users/chuntengxiao/.shea-symphony/mvp/shea-symphony-2606-mvp/app
SHEA_SYMPHONY_APP_PROFILE_PATH=/Users/chuntengxiao/Documents/GitHub/shea-symphony/.shea/app-profile.local.json npm run tauri -- dev
```

The profile points at the Shea Symphony target checkout and the MVP CLI binary.
Do not assume `npm run tauri -- dev -- --workdir <path>` alone keeps the backend
on MVP code.

Default assignee:

```text
Alive24
```

## Operating Rule

Conversation and draft repair live in this skill. Deterministic validation and
tracker mutation live in the Shea Symphony CLI.

Follow this order:

1. Understand the rough intent.
2. Identify grey areas that affect execution.
3. Ask 1-3 focused questions in natural language.
4. Ask another short clarification round while useful ambiguity remains.
5. Draft the issue contract.
6. Ask for explicit operator confirmation before creating or promoting.
7. Validate with `shea-symphony forge validate`, create with
   `shea-symphony forge create`, or route Human Review contract revisions with
   `shea-symphony forge rework` after confirmation.
8. If the gate returns `NeedToClarify`, repair only the missing pieces and retry.
9. Report the issue URL, number, Project status, and any dogfood findings.

For a live issue already in `Human Review` whose execution contract must
change, do not use `forge promote` or raw Project mutation. Discuss the revised
scope with the operator, prepare a full replacement Rework body and evidence
file, require explicit confirmation, then run `forge rework`. The CLI stays
non-interactive and owns the guarded body/evidence/status writes.

## Discuss Flow

- Act as a thinking partner, not a form.
- Ask only questions that affect downstream execution.
- Offer recommended answers when the user has already implied a direction.
- Do not ask about low-level implementation details unless the issue goal
  depends on them.
- Capture deferred ideas separately instead of bloating the issue.
- Stop asking only when the user explicitly says to stop, skip, hand off, draft,
  create, or proceed.
- Always tell the user they can skip remaining questions and proceed to handoff
  if the remaining ambiguity is acceptable.
- If the user skips, record reasonable assumptions in the draft.
- Always evaluate whether the candidate should be split into a native
  parent/subissue batch before drafting. Ask this explicitly when the work spans
  multiple independently testable implementation slices, touches multiple lanes
  or operator surfaces, carries high review risk, or would otherwise create one
  oversized PR.
- If the operator chooses a parent/subissue batch, draft the parent as the final
  Human Review and UAT owner. Draft subissues as implementation slices that
  still require independent Agent Review but do not require routine direct UAT
  or direct Human Review. Use `Subissue Human Review Exception: <reason>` only
  when a child truly needs direct Human Review.
- Do not create a dispatchable `Todo` issue that relies only on body text for a
  blocking dependency. If the workflow cannot create a structured blocked-by
  relationship in the same creation flow, recommend `Backlog` until the blocker
  is Done, then promote it to `Todo`.

## Investigation And Boundary Scan

Before drafting issues for migrations, backend changes, external CLI/tool
integrations, workflow orchestration, protocol changes, or other abstraction
boundary work, do a short investigation pass before asking implementation-shape
questions.

The investigation should separate:

- external facts: current official docs, CLI help, version output, or safe
  compatibility probes when the behavior is likely to have changed;
- local implementation facts: current config fields, backend abstractions,
  prompt/runtime paths, docs, skills, tests, and operator readback surfaces;
- boundary facts: which concepts are accidentally coupled today and which
  behaviors must remain stable while the issue is implemented.

For external tools, prefer official docs and local `--help` / non-sensitive
smoke probes over assumptions. Do not send private repository contents to an
external service during investigation unless the operator explicitly approves
that exposure. A safe probe should use synthetic text, write logs under an
artifact or temp path when possible, and record whether the result was observed
locally or inferred from docs.

For migration or adapter work, first ask whether the real issue is an
abstraction boundary rather than a command-name replacement. Identify the
existing coupled concerns, such as config schema, transport, parser, health
diagnostics, artifact/ledger shape, docs, skills, and default workflow config.
Prefer a conservative issue sequence when useful:

1. generalize the internal abstraction without changing the default behavior;
2. add or validate the new backend/tool behind the abstraction;
3. switch defaults, docs, skills, and operator guidance after compatibility is
   proven.

Only draft a single large issue when the scan shows the work is small enough to
verify safely in one PR.

Resolve these before creation:

- Goal.
- Why now.
- Parent/subissue shape: single issue or native parent/subissue batch; if
  batched, identify the parent contract, child slices, parent-owned UAT, and any
  direct child Human Review exceptions.
- Target Repository / Package, usually `Alive24/shea-symphony`.
- Scope and out-of-scope boundaries.
- Non-negotiable guardrails.
- Dependencies, with explicit `None` when there are none.
- Trusted docs/code references.
- Investigation evidence for abstraction-boundary, migration, or external-tool
  work, including what was checked and what remains unverified.
- Comment expectations for non-obvious runtime, tracker, schema, retry,
  idempotency, compatibility, or external-service boundaries. For boundary-heavy
  issues, include this in `Non-Negotiable Guardrails` and make it objectively
  checkable in `Completion Criteria`.
- Verification commands. Prefer:
  - `cargo test`
  - `cargo fmt --check`
  - `cargo clippy --all-targets --all-features -- -D warnings`
- UAT requirements for operator-facing surfaces.

## Issue Body Shape

Use this structure:

```md
## Issue Setup

- UAT Required: Yes / No
- Assignee: Alive24
- Dependencies: None / [specific blocker relationship semantics]
- Related Parent Issue or Context: ...

## Issue Goal

...

## Issue Context

...

### Why Now

...

### Target Repository / Package

- Alive24/shea-symphony

## Non-Negotiable Guardrails

- ...
- Add concise comments at non-obvious runtime, tracker, schema,
  retry/idempotency, compatibility, or external-service boundaries changed by
  this issue.

## Scope

### In Scope

- ...

### Out of Scope

- ...

## Canonical References

### Relevant Knowledge Sources

- `docs/...`

### Relevant Code Paths

- `src/...`

### External References

- https://...

## Current State

...

### Code-State Freshness

...

## Deliverable Shape

...

## Risks or Constraints

- ...

## Expected Outcome

- [ ] ...

## Verification

### Completion Criteria

- [ ] ...
- [ ] Non-obvious boundary code added or changed by this issue has concise
      comments explaining the replay, side-effect, schema, retry/idempotency,
      compatibility, or external-service constraint.

### Functional Verification

- [ ] `cargo test`

### UAT

- [ ] ...

### Context Verification

- [ ] Confirm the issue still matches latest `main`, relevant open PRs, and
      recently completed work before dispatch.
```

Only include `External References` when needed. Do not put explanatory text
before an external URL in `Relevant Knowledge Sources`; the quality gate treats
that section as local path-like references.

The `Expected Outcome`, `Completion Criteria`, `Functional Verification`,
`UAT`, and `Context Verification` sections must use Markdown checkboxes. These
checkboxes are the Review Agent evidence checklist; write each item so it can be
objectively checked or left unchecked from PR diff, Main Workpad evidence,
timeline comments, command output, or operator evidence.

## Creation Workflow

After the user confirms the draft:

1. Write the issue body to `/private/tmp/<slug>.md`.
2. Run:

```bash
cd /Users/chuntengxiao/.shea-symphony/mvp/shea-symphony-2606-mvp
cargo run -- forge create \
  --workflow workflows/shea-symphony.md \
  --title "<title>" \
  --body-file /private/tmp/<slug>.md \
  --status Todo \
  --assignee Alive24 \
  --write
```

3. If the gate returns `NeedToClarify`, repair only the missing pieces and retry.
4. Read back the created issue through the Shea Symphony CLI or ordinary
   `gh issue view` for raw issue content.

For `forge create`, the Project status assignment is part of creation and should
be the final mutating action for that issue. Prepare the complete body file and
operator-confirmed title first; after creation, only read back and report.

## Safety

- Never create tracker issues without explicit user confirmation unless the user
  directly says to create it.
- Never bypass the Issue Quality Gate by using raw `gh issue create`.
- Do not mutate code while using this skill.
- Keep temporary issue-body files under `/private/tmp`.
- If GitHub or Project reads fail due network or rate limits, explain and stop
  before creating duplicates.
