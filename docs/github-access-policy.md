# GitHub Access Policy

Shea Symphony CLI is the authority for workflow-critical GitHub access. Project
state, Project fields, relationships, claim locks, workpads, workflow status,
and linked-PR handoff checks must go through CLI commands or shared tracker
helpers.

## Access Rules

- Prefer REST-backed `gh api` reads when REST exposes the required issue, PR, or
  repository fact.
- Prefer REST-backed ProjectV2 metadata, field, item, and supported item-field
  update paths when GitHub exposes the required data. Use GraphQL only for
  Project relationships, issue/comment mutations that require node IDs, item
  addition, or explicit REST capability gaps.
- Set `tracker.project_owner_type` to `user` or `organization` when the Project
  owner type is known. Omit it only for legacy organization-first/user-fallback
  workflows.
- Keep ProjectV2 GraphQL shallow: small page sizes, explicit field selection,
  split metadata from item-page queries, and paginate with cursors.
- Keep high-frequency queue scans distinct from rich targeted issue reads:
  queue scans should carry lane-safe Project fields and gate data, while issue
  body, comment/workpad, linked-PR, and detailed topology hydration belongs to
  `project issue`, `project inspect`, or targeted Doctor/lane diagnostics.
- Route GitHub CLI execution through `GithubCliAccess` in `src/tracker.rs` so
  retry, JSON parsing, auth, rate-limit, transient-backend, resource-limit,
  partial-response, and missing-capability diagnostics stay consistent.
- Raw `gh issue view` or `gh pr view` is allowed for read-only diagnostics when
  the CLI lacks the needed content read. Record it as a CLI gap in workpad or
  report evidence when it affects workflow decisions.
- Raw `gh project`, Project GraphQL, or Project UI writes are break-glass only.
  Record the missing CLI surface and preserve evidence before any such repair.

## Current Inventory

| Surface | Current usage | Classification | Target |
| --- | --- | --- | --- |
| `src/tracker.rs` ProjectV2 metadata, field, item, and supported item-field updates | REST `gh api .../projectsV2/...` through tracker helper methods, with GraphQL fallback for gaps | Must stay behind CLI helper | Cache Project id, field ids, Status option ids, and supported field metadata process-locally; parse paginated REST output; keep fallback reasons explicit. |
| `src/tracker.rs` ProjectV2 rich issue reads, item addition, workpad/comment mutations, and unsupported field updates | `gh api graphql` through tracker helper methods | GraphQL fallback or required node-id path | Keep page sizes small, field selection explicit, and fallback reasons visible in code/tests/operator diagnostics. |
| `src/tracker.rs` native blocker reads | REST `gh api repos/{owner}/{repo}/issues/.../dependencies/blocked_by` | Preferred REST read | Keep REST-first helper path. |
| `src/tracker.rs` issue edits and assignment | `gh issue edit` from CLI commands | CLI-owned mutation helper | Route through `GithubCliAccess::run_status` for consistent diagnostics. |
| `src/tracker.rs` workpad comments, issue creation, PR linkage comment, issue close | GraphQL mutations | GraphQL required or currently node-id-shaped | Keep inside CLI; do not expose raw operator commands. |
| `src/main.rs` lane prompt warnings and Project mutation audit | Operator-facing guard text | Must stay CLI-first | Keep warning text aligned with this policy. |
| `src/git_handoff.rs` PR read commands | `gh pr view` for handoff evidence | Allowed helper-level PR read | Keep behind CLI helper surface; avoid operator raw Project reads. |
| `.codex/skills/` | Doctor/Main/Review/Merge manual workflows mention raw `gh` fallbacks | Allowed only as labeled CLI-gap diagnostics | Prefer grouped CLI commands and require workpad/report evidence for fallbacks. |
| `docs/cli-command-reference.md`, `docs/operator-dogfood.md`, `docs/operator-doctor.md` | Operator guidance for CLI vs raw GitHub access | Policy documentation | Keep raw reads diagnostic and Project writes break-glass. |
| `workflows/prompts/*.md` | Lane authority contracts | Workflow-critical | Route Project facts and mutations through Shea Symphony CLI. |

## Missing Or Deferred CLI Surfaces

- Ordinary raw issue/PR body and comment reads still rely on `gh issue view` or
  `gh pr view` in some manual diagnostics. That is an allowed CLI gap until a
  dedicated content-read command exists.
- PR diff inspection is outside the Project state machine. Review and Merge may
  use ordinary Git/GitHub PR reads when local checkout or review context needs
  them, but linked-PR state and draft/ready gates must be verified through CLI
  readback.
- ProjectV2 still requires GraphQL for rich issue/comment relationship reads,
  adding items to Projects by node ID, and REST capability gaps. This policy
  optimizes and centralizes GraphQL; it does not remove it.
