---
name: shea-symphony-doctor
description: Diagnose concrete Shea Symphony failures, repository-contract problems, operational inconsistencies, interrupted workflows, Need Human Input items, and issue or PR blockers; propose the smallest evidence-backed repair and apply only an explicitly confirmed bounded change. Do not use for routine project checkpoints, progress summaries, next-work prioritization, or Backlog mining; use shea-symphony-issue-forge-reflect for those.
---

# Shea Symphony Doctor

Use read-first diagnosis for Doctor findings, repository-owned workflow and
agent contracts, debug output, operational inconsistencies, stuck `Need Human Input` items,
and issue or PR blockers. Keep Doctor outside normal Main, Review, Merge, and
Issue Forge authority.

Routine project checkpoints, progress summaries, next-work prioritization, and
Backlog mining belong to `$shea-symphony-issue-forge-reflect` when no concrete
malfunction is reported. If a checkpoint exposes one concrete failure,
diagnose only that bounded symptom; do not convert the whole checkpoint into
Doctor triage.

## Bind the Active Repository

Discover the target repository root, selected app profile, workflow, CLI,
tracker, configured lane prompts, workpad templates, repo-owned Shea skills,
and relevant issue, PR, session, worktree, or run evidence. Never assume a
checkout path, workflow filename, executable, model, or harness.

Treat these boundaries separately:

- repository-owned workflow, prompts, workpad templates, and repo-local skills
  are eligible for inspection;
- rendered prompt and runtime-envelope readback is evidence, but CLI-owned
  runtime envelopes and tracker mutation mechanics are not editable contracts;
- global or separately installed skills are outside a repository-local repair;
- ordinary issue and PR reads may use the configured provider, while configured
  workflow surfaces remain authoritative for status, claims, and relationships.

If a binding cannot be proven, report it as the finding and ask for the
smallest missing fact. Do not guess an editable path.

## Diagnose and Recommend

For every Doctor finding, report:

- exact observed evidence and whether it is a blocker or warning;
- affected issue, PR, worktree, session, contract, or repository-local skill path;
- inference, confidence, and plausible alternatives, kept separate from facts;
- the smallest workflow-owned repair path;
- whether the repair is safe in this session; and
- the one operator decision still needed, if any.

End with one concrete next step: a named lane handoff, the configured foreground
workflow action, a documented state/PR/worktree/skill-loading repair, a
`repository_contract_repair` plan, or one focused operator question.

## Repository Contract Repair

Use `repository_contract_repair` only for evidence-backed problems in contracts
owned by the bound repository. Read
[`references/repository-contract-repair.md`](references/repository-contract-repair.md)
for the required plan and evidence formats. Load the files under
`fixtures/repository-contract-repair/` only when validating or forward-testing
this capability.

### 1. Establish evidence

Read the configured workflow, lane prompts, workpad templates, repo-owned Shea
skills, rendered prompt/runtime-envelope readback, referenced files, and
relevant run evidence. Record the current Git state and the bytes or hashes of
candidate paths before proposing a write.

Separate `Observed evidence` from `Doctor inference`. Choose one or more of
these classifications:

- `missing_completion_invariant`
- `duplicated_instruction`
- `contradictory_instruction`
- `stale_or_unreachable_text`
- `wrong_layer_instruction`
- `lane_leakage`
- `excessive_procedure`
- `unused_workpad_structure`
- `unsafe_simplification`
- `no_change`

Length and repetition are signals, not proof of poor behavior. Confirm how a
model, harness, later lane, recovery path, or external tool consumes the text
when that affects safety. State uncertainty instead of inventing causality.

### 2. Preserve invariants

Before proposing a repair, identify every invariant the affected contract must
retain. At minimum, audit:

- lane authority and stop boundaries;
- claim, ownership, worktree, and recovery identity;
- required verification and in-scope self-repair;
- PR readiness, native linkage, evidence, and handoff obligations;
- independent Review and operator-owned Human Review;
- status as the final tracker mutation at a lane boundary;
- workflow-configurable prompts/workpads versus CLI-owned runtime envelopes;
- path-scoped confirmation, unrelated-change preservation, and rollback.

For Main completion, preserve this behavior even if its wording is simplified:
repair in-scope lint, format, type, build, or test failures; rerun affected
verification; and do not report completion until required verification,
ready-PR, linkage, workpad, and Agent Review handoff obligations pass.

Refuse any proposal that removes the only effective authority, safety, claim,
verification, PR, review, or state-boundary rule. Classify it as
`unsafe_simplification` and produce no writable diff.

### 3. Produce a Contract Repair Plan

Prefer removal, consolidation, relocation, or shorter wording when behavior
remains safe. Add text only when observed evidence shows an execution-critical
boundary is missing.

The plan must include the observed failure, inference and confidence, affected
lane/model/harness when known, exact paths, proposed removals/merges/
relocations/additions, preserved invariants, expected improvement,
verification, rollback, and explicit `no_change` or refusal when applicable.

Show a focused unified diff before writing. List the complete allowed path set.
Do not turn a named-path proposal into a workflow-wide or suite-wide rewrite.

### 4. Obtain path-scoped confirmation

Ask the operator to confirm the exact paths and displayed diff. A request that
already names the repair and paths confirms only that bounded proposal. Any
material diff or path expansion requires new confirmation.

Confirmation does not authorize tracker state changes, global skill installs,
commits, pushes, PR creation, issue promotion, or unrelated cleanup. Do not
write when confirmation is ambiguous.

### 5. Apply the bounded edit

Re-read the approved paths immediately before writing and stop if their bytes
changed. Apply only the confirmed diff to repository-owned paths. Preserve
target customizations and unrelated dirty files; verify that the changed-path
set is a subset of the confirmed set.

Do not add an autonomous optimizer, opaque quality score, model-specific
universal rule, production self-modifying loop, or silent automatic rewrite.

### 6. Validate and record evidence

Run the strongest repository-owned checks for the edited contract, including
as applicable:

- workflow parsing and referenced-file resolution;
- lane-prompt and workpad-template rendering with required variables;
- runtime-envelope readback without editing the envelope;
- repository-local skill frontmatter, metadata, referenced resources, and
  discovery;
- focused fixture checks and relevant repository tests; and
- changed-path and unrelated-byte comparison.

If validation fails, repair only within the confirmed diff or roll back the
bounded edit. Do not claim success with a repairable in-scope failure.

Append standalone `Shea Symphony Doctor Contract Repair` evidence through the
configured timeline surface. Do not use or overwrite the persistent Main Agent
Workpad. Record before/after paths, diagnosis, applied diff summary, validation,
preserved invariants, unchanged-path evidence, rollback guidance, and outcome.
Repository-contract repair itself must not change Project status.

Return exactly one disposition: `repaired`, `no_change`, `refused_unsafe`,
`confirmation_needed`, or `blocked`.

## Other Confirmed Repairs

When the operator already requested a specific bounded recovery, that request
confirms only that recovery. Print the target paths or tracker mutation before
writing and do not expand the scope.

A non-contract recovery is bounded only when it keeps the selected issue scope,
reuses or safely reconciles existing work, makes the smallest change needed to
restore a valid boundary, and stops before independent Review, Human Review, or
merge authority. Read back every external mutation.

## Evidence and Hard Boundaries

- Record ordinary diagnosis and recovery as an append-only
  `Shea Symphony Doctor Triage` timeline note; use the Contract Repair title for
  `repository_contract_repair`. Never overwrite the Main Agent Workpad.
- Do not start normal Main, Review, Human Review, or Merge work from Doctor.
- Do not change Project state without separate explicit authorization for the
  documented repair. Contract repair never changes Project state.
- Vendored repository skills are owned by that repository. Do not compare them
  with upstream text or versions, overwrite them silently, or treat intentional
  customization as drift. Use only a targeted, confirmed repository-local repair.
- Inspect grouped workspace/session evidence and local Git metadata before
  repairing ambiguous ownership.
- Never merge a PR or make an independent acceptance decision.
