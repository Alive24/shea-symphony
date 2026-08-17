---
name: shea-doctor
description: Diagnose concrete Shea Symphony tracker, repository-contract, workspace, runtime, and lane-boundary failures; propose or apply only bounded repairs with evidence and explicit authority.
metadata:
  short-description: Diagnose and repair Shea safely
---

# Shea Symphony Doctor

Doctor diagnoses and repairs known invariants. It is not an implementation, Review, Human approval, merge, cleanup, or self-modification authority.

## Resolve and diagnose

Read `.shea/contracts/workflow-capability.v1.md`, resolve the active workflow, and select a supported adapter. Prefer targeted reads for a named issue; use Project-wide audit only when the concrete ambiguity requires it. Separate **Observed evidence** from **Doctor inference** and report quota, permission, backend, and uncertain-write failures exactly.

Before any repair, state the violated invariant, affected issue/files, allowed path set, intended evidence, exact mutation, readback, and refusal boundary. Evidence is durable before Project state, and state is the final mutation.

Doctor never deletes worktrees, discards local work, clears a live claim speculatively, fabricates Review/Human evidence, merges, or changes an issue contract as a shortcut.

Runtime-profile drift, missing repository execution requirements, harness
selection, or broader setup reconciliation routes to `setup-shea`. Doctor may
diagnose and preserve the evidence, but must not select an environment, rewrite
the runtime profile, or restore repository resources from upstream.

## Repository contract repair

Use `repository_contract_repair` only for repository-owned Markdown contracts such as normal operational Skills, prompts, workpads, capability references, and operator docs. Read `references/repository-contract-repair.md` and classify findings as:

- `missing_completion_invariant`
- `duplicated_instruction`
- `contradictory_instruction`
- `lane_leakage`
- `stale_or_unreachable_text`
- `unsafe_simplification`
- `no_change`

Produce Observed evidence, Doctor inference, the complete allowed path set, preserved authority invariants, and verification. Show a focused unified diff before writing. Refuse edits outside the confirmed set or any proposal that weakens confirmation, fail-closed behavior, targeted reads, state-last ordering, independent Review, Human authority, PR linkage, or recovery evidence.

In this mode, runtime envelopes and tracker mutation mechanics are not editable contracts. Repository-contract repair itself must not change Project status.

Vendored repository skills are owned by that repository. Do not compare them
  with upstream text or versions, overwrite customization, or recreate package-manager/parity behavior.

## Bounded operational repairs

Doctor may record diagnostics, restore an issue incorrectly placed in Human Review without independent PASS back to Agent Review, or mark a verified ready PR through an explicitly confirmed supported repair. Use the selected adapter's guarded actions and targeted readback; preserve append-only Doctor evidence and never overwrite the canonical Main workpad.
