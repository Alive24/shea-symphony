---
name: shea-symphony-manual-main
description: Execute one operator-selected Shea Symphony Main-lane issue now, including guarded Todo, authorized Backlog, resumable In Progress, or Main Rework implementation through a ready PR and Agent Review handoff.
metadata:
  short-description: Execute one manual Shea Main issue
---

# Shea Symphony Manual Main

Execute one operator-selected Main issue in the current task. Do not create another task, delegate the implementation, review your own result, or merge it.

## Resolve mechanics

Read `.shea/contracts/workflow-capability.v1.md`, resolve its `active_workflow`, and select a supported adapter such as `.shea/contracts/adapters/legacy-cli.v1.md`. The capability contract owns targeted reads, guarded-action ordering, uncertain writes, and readback; the adapter owns syntax. Fail closed when either reference or a required capability is unavailable.

The active workflow owns repository, Project, states, base/target branch, workspace root, verification, templates, and backend policy. Machine-local profiles own executable paths. Do not hard-code either.

## Authority and selection

Main owns Todo implementation, Main-lane Rework, a consistently resumable In Progress claim, and an operator-named Backlog issue explicitly authorized for direct execution. It does not own ordinary Backlog shaping, independent Agent Review, Human Review decisions, Merging, or merge-lane repair.

Before any mutation, use `issue.read`, `issue.inspect`, `relationships.read`, `evidence.read`, and `pull_request.read` as narrowly as possible. Require:

- an allowed state and an empty or matching Main claim;
- a `Ready` or `ReadyWithAssumptions` quality gate;
- terminal blockers and Done native subissues;
- an implementable contract with an unambiguous target branch;
- one consistent issue/workspace/branch/PR identity.

For direct Backlog execution, also validate the current title/body at Todo quality without promoting it. Keep it Backlog and skip `lane.claim`/In Progress until the final ready handoff. Otherwise route an incomplete contract to Need to Clarify and a credential, sample, authority, or product-decision blocker to Need Human Input.

## Workspace, claim, and plan

Inspect registered worktrees, existing evidence, claims, sessions, branches, and linked PRs before acting. Reuse or adopt the one safe canonical issue worktree; an isolated harness worktree may be adopted when clean and conflict-free. Never implement in the canonical checkout or create a nested/replacement worktree while a valid candidate exists.

Apply `workspace.adopt`, then `lane.claim`, then read back each effect. Upsert exactly one canonical `Shea Symphony Workpad` with an issue-specific checkbox plan, run identity, workspace origin, and claim/readiness evidence. For ordinary Main work, transition to In Progress only after that evidence is durable; Project state is the phase's final mutation.

## Execute

Implement only accepted scope. Maintain the existing canonical Main workpad in place across resume and Rework; update stable Plan, Work Log, Verification, PR / Linkage, Run Identity, Recovery / Rework, and Handoff sections without erasing prior evidence. Append-only Review, Human Review, Doctor, and Merge records remain separate.

Run the strongest repository-owned checks for the touched area and repair in-scope failures. For changed Rust public API, add semantic Rustdoc, audit visibility, and run strict Rustdoc. Record changed files, commands/results, risks, compatibility, boundary comments, and Rustdoc/public-visibility evidence.

## Publish and hand off

Commit and push the single issue branch, then create or update one ready, non-draft PR against the confirmed target. Default-base PRs must use a native closing relationship and read back as `source=github_native`. A non-default-base diagnostic may be `source=fallback_diagnostic` only when the contract/operator explicitly accepts it; never describe fallback evidence as native.

Complete the canonical workpad with commit, PR URL, bases, readiness, exact link source, verification, and handoff summary. Apply `issue.transition` to Agent Review only after all evidence is durable and only as the final mutation; then perform readback only.

Main never sets Human Review, uses the Merging Agent field, merges a PR, weakens lane/confirmation boundaries, hides quota or backend failures, or treats a partial prompt as completion.
