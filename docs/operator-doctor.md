# Shea Symphony Doctor Operator Spec

Status: Doctor v2

The Shea Symphony Doctor skill is a repo-owned Codex operator workflow for
diagnosing stuck tracker, PR, claim, worktree, runtime, and skill-install
symptoms. It is intentionally read-first and confirmation-gated.

Doctor v1 complements the existing `doctor`, `project state`, `project issue`,
`debug`, `workspace`, and `session` commands. It does not replace them. After
diagnosis, it must give a concrete repair recommendation and may execute the
confirmed repair in the same Codex session when the workflow contract allows it.

Doctor v2 also defines `repository_contract_repair` for repository-owned
workflow, lane-prompt, workpad-template, and skill contracts. It remains a
read-first, confirmation-gated skill workflow rather than an automatic CLI
rewriter.

## Entry Points

Use Doctor v1 when:

- an issue is already in `Need Human Input`.
- an operator selects a specific issue or PR for diagnosis.
- a lane handoff cannot proceed because linked PR, draft PR, claim, session, or
  runtime evidence is incomplete.
- cleanup could discard useful worktree or runtime evidence.
- a repository-local Shea Skill cannot be discovered or loaded as configured;
  Doctor may diagnose the concrete contract but does not restore it from an
  upstream copy.
- observed runs or repository contracts show missing completion boundaries,
  duplicated or contradictory instructions, wrong-layer text, lane leakage,
  excessive procedure, unused workpad structure, or a likely safe
  simplification.

Doctor v1 should not scan or mutate the whole Project by default when the
operator selected one issue. Whole-Project reads are allowed only as context for
health and invariant checks.

## Safety Model

Doctor v1 defaults to:

1. read live Project and local runtime state.
2. preserve evidence.
3. classify the stuck state.
4. recommend one small executable repair path.
5. state whether it can be executed in the current session.
6. list any repair actions that still need explicit confirmation.

Doctor v1 must not perform automatic Project mutation, PR linkage repair, stale
claim repair, worktree cleanup, runtime cleanup, repository-local skill writes, or PR ready
transitions. Those actions require an explicit operator request, an explicit
operator confirmation, or a documented command with a `--write` flag.

Shea Symphony CLI remains the normal authority for Project status, claim locks,
relationships, workpads, and workflow status. Raw `gh project` or GraphQL writes
are break-glass repairs only when the CLI lacks a surface for the exact repair;
the triage note must record why the break-glass path was used.

Repository-contract repair has a stricter boundary: it never mutates Project
status, and operator confirmation covers only the exact displayed paths and
diff. It does not authorize commits, pushes, PR creation, issue promotion,
changes outside the repository, or unrelated cleanup.

## Repository Contract Repair

Use the canonical Doctor skill's `repository_contract_repair` path. Resolve the
active repository and configured workflow first, then inspect:

- configured lane prompts and workpad templates;
- repository-owned Shea Skills and their referenced resources;
- rendered prompt and runtime-envelope readback;
- required variables and referenced files; and
- relevant run, model, harness, Review, and recovery evidence.

Runtime-envelope readback is evidence only. CLI-owned runtime envelopes,
tracker mechanics, and separately installed/global skills are not editable
repository-agent contracts.

### Diagnosis

Every diagnosis separates `Observed evidence` from `Doctor inference`, includes
confidence and alternatives, and uses one or more of:

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

Prompt length or duplication alone is not a failure. Confirm the behavior or
consumer affected when that fact changes the repair. A single model or harness
run is evidence for that context, not a universal preference.

### Plan, preview, and confirmation

Produce the `Shea Symphony Contract Repair Plan` from
`.agents/skills/shea-doctor/references/repository-contract-repair.md`.
It records the observed failure, inference/confidence, affected lane/model/
harness, exact paths, removals/merges/relocations/additions, preserved
invariants, expected improvement, verification, rollback, and confirmation
boundary.

Prefer subtraction, consolidation, relocation, or shorter wording. Add one
concise rule only when evidence shows an execution-critical boundary is
missing. Show a focused unified diff and the complete allowed path set before
writing. Refuse a simplification that removes the only effective authority,
safety, claim, verification, PR, review, or state-transition invariant.

For Main completion, preserve the behavior that repairable in-scope lint,
format, type, build, or test failures are fixed and rerun, and that completion
waits for required verification, a ready linked PR, workpad evidence, and Agent
Review handoff.

### Apply and validate

After exact path-and-diff confirmation, re-read the approved bytes and stop on
drift. Apply only the confirmed diff. Then validate workflow parsing,
referenced files, prompt/workpad rendering and variables, runtime-envelope
readback, skill frontmatter/metadata/manifest, installer dry-run/validation,
fixture expectations, and the changed-path subset. Compare unrelated target
customizations byte-for-byte.

Do not add an autonomous optimizer, opaque score, production self-modifying
loop, automatic rewrite, model-specific universal rule, or tracker mutation.
If validation fails, repair only inside the confirmed diff or roll it back.

### Durable evidence and outcomes

Append the `Shea Symphony Doctor Contract Repair` evidence format from the same
reference file through the configured timeline surface. Never use or overwrite
the persistent Main Agent Workpad. Record the confirmed paths, before/after
summary, validation, preserved invariants, unchanged-path evidence, rollback,
and `Tracker state: unchanged`.

Return one outcome: `repaired`, `no_change`, `refused_unsafe`,
`confirmation_needed`, or `blocked`.

## Required Evidence Reads

For all Doctor sessions:

```bash
cargo run -- project state workflows/shea-symphony.md
cargo run -- doctor workflows/shea-symphony.md
cargo run -- debug workflows/shea-symphony.md
```

For a selected issue:

```bash
cargo run -- project issue workflows/shea-symphony.md '#258' --json
cargo run -- doctor workflows/shea-symphony.md repair '#258'
```

For an issue that might still be dispatchable:

```bash
cargo run -- project inspect workflows/shea-symphony.md '#258'
```

For worktree or session ambiguity:

```bash
cargo run -- workspace show workflows/shea-symphony.md '#258'
cargo run -- session list workflows/shea-symphony.md
git worktree list --porcelain
```

For ordinary PR or issue text:

```bash
gh issue view 258 --repo Alive24/shea-symphony
gh pr view 123 --repo Alive24/shea-symphony --json number,isDraft,url,state
```

Use those raw reads only as diagnostic content fallbacks when the CLI lacks the
needed issue or PR content surface, and record the CLI gap when the result
changes the repair recommendation. Do not use raw Project reads for status,
claim locks, dependency relationships, workpads, or linked PR state in normal
Doctor flow. See `docs/github-access-policy.md` for the current inventory and
fallback classification.

## Classification

Every Doctor session should choose one primary classification:

| Classification | Meaning | Typical next step |
| --- | --- | --- |
| `need_human_decision` | Continuation depends on a decision, credential, missing sample, destructive approval, or unstated product choice. | Ask one concrete question or move/preserve evidence in `Need Human Input`. |
| `missing_pr_linkage` | A PR exists or is expected, but `project issue` does not expose it as linked. | Preserve PR URL evidence and request confirmation for linkage repair. |
| `draft_pr_handoff` | A linked PR is draft before Agent Review handoff. | Use documented ready repair only after confirmation. |
| `stale_lane_claim` | A lane claim is stale, mismatched, failed, superseded, or missing registry evidence. | Preserve prior claim and request confirmation before superseding it. |
| `dirty_runtime_or_worktree` | Runtime state, session registry, or issue worktree is dirty or ambiguous. | Inspect, preserve evidence, and avoid cleanup until confirmed. |
| `skill_loading_symptom` | A repository-local Skill path, frontmatter, metadata file, or referenced resource is concretely broken. | Diagnose the repository-owned contract and propose a targeted confirmed repair without comparing it to upstream text or versions. |
| `issue_contract_gap` | The issue contract lacks execution-critical scope, verification, or dependency facts. | Move or recommend moving to `Need to Clarify`. |
| `no_repair_needed` | Evidence shows the item is healthy. | Return to the normal lane flow. |

Secondary classifications are useful only when they change the repair prompt or
lane handoff.

## Doctor Triage Note

Use this format for durable append-only issue timeline evidence. Write
operator-authored notes with `project timeline-comment`; do not use
`project workpad`, which is reserved for the persistent Main Agent Workpad.

```markdown
## Shea Symphony Doctor Triage

- Issue: `#258`
- Lane: `doctor`
- Actor role: `doctor`
- Run ID: `<doctor-action-id>`
- Status at triage: `Need Human Input`
- Input state: `Need Human Input`
- Target state after repair: `Need Human Input` | `Agent Review` | `unchanged`
- Result: Routed | Repaired | Triage recorded | Blocked
- PR: #<pr> <url> | `not recorded`
- Evidence summary: ...
- Primary classification: `missing_pr_linkage`
- Secondary classifications: `stale_lane_claim`
- Diagnosis: The issue is blocked because ...
- Evidence read:
  - `cargo run -- project state workflows/shea-symphony.md`: `trusted=true`
  - `cargo run -- project issue workflows/shea-symphony.md '#258' --json`: ...
  - `cargo run -- doctor workflows/shea-symphony.md repair '#258'`: ...
- Recommended next step: ...
- Explicit repair recommendation: ...
- Can execute in this session: `yes` | `no`
- Repair actions requiring explicit confirmation:
  - ...
- Safe no-write commands to run next:
  - ...
- Related follow-ups:
  - Record only concrete repository-local Skill loading or contract problems; vendored customization is not drift.
```

The note must separate observed evidence from inference. If evidence is stale,
say so.

## Confirmation-Gated Repairs

These repairs require explicit confirmation or a documented `--write` path:

- Project status changes, including moves to `Need Human Input`.
- lane claim repair, superseding claims, or clearing claims.
- PR linkage repair.
- marking draft PRs ready.
- archiving, deleting, or cleaning worktrees.
- runtime state cleanup.
- repository-local Skill contract writes.

Prefer the smallest repair that preserves issue, PR, worktree, session, and
workpad context. Do not reset when repair can preserve evidence.

If the operator has already requested the specific repair in the current
conversation, such as updating the local Doctor skill, that request satisfies
the confirmation gate for that bounded target. Still print or record the target
paths and do not broaden the write to unrelated skills.

## Same-Session Execution

Doctor diagnosis may continue into repair work in the original Codex session
when all are true:

- the target issue, PR, worktree, or local skill path is known.
- prior lane evidence remains valid or the owning lane workflow is adopted.
- the action has explicit operator confirmation or a documented `--write` path.
- durable evidence will be recorded before terminal Project routing changes.

Use the owning skill or lane workflow before doing normal Main, Review, Human
Review, or Merging work. Doctor coordinates the repair path; it does not turn a
diagnosis-only step into hidden implementation or merge authority.

## Repository-Owned Skill Boundary

Doctor may diagnose a concrete repository-local Skill discovery, frontmatter,
metadata, or referenced-resource failure. It treats the repository's vendored
files as authoritative and must not compare them with upstream text or versions,
restore them from a source copy, or interpret intentional customization as drift.
A targeted repository-local repair still requires the normal confirmation gate.

## Outcomes

End every Doctor run with one outcome:

- `resume_main`: normal Main Agent flow should resume.
- `resume_review`: Review Agent flow should resume.
- `resume_merge`: Merging Agent flow should resume.
- `operator_confirmation_needed`: one explicit repair decision is needed.
- `need_to_clarify`: the issue contract must be repaired before dispatch.
- `need_human_input`: durable evidence is preserved and human input remains the
  next blocker.
- `no_action`: no stuck state remains.

Doctor v1 must never move an issue to `Human Review` and must never merge a PR.
