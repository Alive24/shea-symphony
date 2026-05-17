---
name: jade-symphony-issue-forge
description: Use when creating, shaping, or validating Jade Symphony GitHub issues from rough operator intent. Runs a conversation-first discuss flow, resolves gate-critical ambiguity, drafts a quality-gated issue, asks for explicit confirmation, then creates it through Jade Symphony forge create.
metadata:
  short-description: Conversational Jade Symphony issue forge
  suite-version: 2026.05.17
---

# Jade Symphony Issue Forge

Create Jade Symphony issues through a conversation-first workflow. Do not jump
straight to `forge create` from rough intent unless the user explicitly provides
a complete issue body.

## Repository

Default repo:

```bash
/Volumes/Bohemialive/GitHub/jade-symphony
```

Canonical workflow:

```bash
workflows/jade-symphony.md
```

Default assignee:

```text
Alive24
```

## Operating Rule

Conversation and draft repair live in this skill. Deterministic validation and
tracker mutation live in the Jade Symphony CLI.

Follow this order:

1. Understand the rough intent.
2. Identify grey areas that affect execution.
3. Ask 1-3 focused questions in natural language.
4. Ask another short clarification round while useful ambiguity remains.
5. Draft the issue contract.
6. Ask for explicit operator confirmation before creating or promoting.
7. Validate with `jade-symphony forge validate` or create with
   `jade-symphony forge create` after confirmation.
8. If the gate returns `NeedToClarify`, repair only the missing pieces and retry.
9. Report the issue URL, number, Project status, and any dogfood findings.

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

Resolve these before creation:

- Goal.
- Why now.
- Target Repository / Package, usually `Alive24/jade-symphony`.
- Scope and out-of-scope boundaries.
- Non-negotiable guardrails.
- Dependencies, with explicit `None` when there are none.
- Trusted docs/code references.
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

- Alive24/jade-symphony

## Non-Negotiable Guardrails

- ...

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

## Deliverable Shape

...

## Risks or Constraints

- ...

## Expected Outcome

- [ ] ...

## Verification

### Completion Criteria

- [ ] ...

### Functional Verification

- [ ] `cargo test`

### UAT

- [ ] ...

### Context Verification

- [ ] ...
```

Only include `External References` when needed. Do not put explanatory text
before an external URL in `Relevant Knowledge Sources`; the quality gate treats
that section as local path-like references.

The `Expected Outcome`, `Completion Criteria`, `Functional Verification`,
`UAT`, and `Context Verification` sections must use Markdown checkboxes. These
checkboxes are the Review Agent evidence checklist; write each item so it can be
objectively checked or left unchecked from PR diff, workpad evidence, command
output, or operator evidence.

## Creation Workflow

After the user confirms the draft:

1. Write the issue body to `/private/tmp/<slug>.md`.
2. Run:

```bash
cd /Volumes/Bohemialive/GitHub/jade-symphony
cargo run -- forge create \
  --workflow workflows/jade-symphony.md \
  --title "<title>" \
  --body-file /private/tmp/<slug>.md \
  --status Todo \
  --assignee Alive24 \
  --write
```

3. If the gate returns `NeedToClarify`, repair only the missing pieces and retry.
4. Read back the created issue through the Jade Symphony CLI or ordinary
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
