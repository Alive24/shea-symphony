# GitHub Access Policy

Jade Symphony CLI is the authority for workflow-critical GitHub access. Project
state, Project fields, relationships, claim locks, workpads, workflow status,
and linked-PR handoff checks must go through CLI commands or shared tracker
helpers.

## Access Rules

- Prefer REST-backed `gh api` reads when REST exposes the required issue, PR, or
  repository fact.
- Use ProjectV2 GraphQL only for Project item reads, Project fields, Project
  mutations, issue/comment mutations that require node IDs, and GitHub surfaces
  that REST cannot provide in the required shape.
- Set `tracker.project_owner_type` to `user` or `organization` when the Project
  owner type is known. Omit it only for legacy organization-first/user-fallback
  workflows.
- Keep ProjectV2 GraphQL shallow: small page sizes, explicit field selection,
  split metadata from item-page queries, and paginate with cursors.
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
| `src/tracker.rs` ProjectV2 queries and mutations | `gh api graphql` through tracker helper methods | Must stay behind CLI helper | Centralized in `GithubCliAccess`; keep small pages and explicit fields. |
| `src/tracker.rs` native blocker reads | REST `gh api repos/{owner}/{repo}/issues/.../dependencies/blocked_by` | Preferred REST read | Keep REST-first helper path. |
| `src/tracker.rs` issue edits and assignment | `gh issue edit` from CLI commands | CLI-owned mutation helper | Route through `GithubCliAccess::run_status` for consistent diagnostics. |
| `src/tracker.rs` workpad comments, issue creation, PR linkage comment, issue close | GraphQL mutations | GraphQL required or currently node-id-shaped | Keep inside CLI; do not expose raw operator commands. |
| `src/main.rs` lane prompt warnings and Project mutation audit | Operator-facing guard text | Must stay CLI-first | Keep warning text aligned with this policy. |
| `src/git_handoff.rs` PR read commands | `gh pr view` for handoff evidence | Allowed helper-level PR read | Keep behind CLI helper surface; avoid operator raw Project reads. |
| `.codex/skills/` | Doctor/Main/Review/Merge manual workflows mention raw `gh` fallbacks | Allowed only as labeled CLI-gap diagnostics | Prefer grouped CLI commands and require workpad/report evidence for fallbacks. |
| `docs/cli-command-reference.md`, `docs/operator-dogfood.md`, `docs/operator-doctor.md` | Operator guidance for CLI vs raw GitHub access | Policy documentation | Keep raw reads diagnostic and Project writes break-glass. |
| `workflows/prompts/*.md` | Lane authority contracts | Workflow-critical | Route Project facts and mutations through Jade Symphony CLI. |

## Missing Or Deferred CLI Surfaces

- Ordinary raw issue/PR body and comment reads still rely on `gh issue view` or
  `gh pr view` in some manual diagnostics. That is an allowed CLI gap until a
  dedicated content-read command exists.
- PR diff inspection is outside the Project state machine. Review and Merge may
  use ordinary Git/GitHub PR reads when local checkout or review context needs
  them, but linked-PR state and draft/ready gates must be verified through CLI
  readback.
- ProjectV2 still requires GraphQL for item fields and mutations. This policy
  optimizes and centralizes GraphQL; it does not remove it.
