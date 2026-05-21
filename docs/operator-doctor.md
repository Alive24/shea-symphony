# Jade Symphony Doctor Operator Spec

Status: Doctor v1

The Jade Symphony Doctor skill is a repo-owned Codex operator workflow for
diagnosing stuck tracker, PR, claim, worktree, runtime, and skill-install
symptoms. It is intentionally read-first and confirmation-gated.

Doctor v1 complements the existing `doctor`, `project state`, `project issue`,
`debug`, `workspace`, and `session` commands. It does not replace them and
does not expand automatic repair authority.

## Entry Points

Use Doctor v1 when:

- an issue is already in `Need Human Input`.
- an operator selects a specific issue or PR for diagnosis.
- a lane handoff cannot proceed because linked PR, draft PR, claim, session, or
  runtime evidence is incomplete.
- cleanup could discard useful worktree or runtime evidence.
- local Jade Symphony skill install symptoms are blocking an operator, while
  `doctor` owns read-only integrity warnings and #242 owns install/update
  writes.
- installable skill suite packaging questions appear, while dated suite
  packaging remains owned by #242.

Doctor v1 should not scan or mutate the whole Project by default when the
operator selected one issue. Whole-Project reads are allowed only as context for
health and invariant checks.

## Safety Model

Doctor v1 defaults to:

1. read live Project and local runtime state.
2. preserve evidence.
3. classify the stuck state.
4. recommend one small next step.
5. list any repair actions that need explicit confirmation.

Doctor v1 must not perform automatic Project mutation, PR linkage repair, stale
claim repair, worktree cleanup, runtime cleanup, or PR ready transitions. Those
actions require an explicit operator confirmation or a documented command with a
`--write` flag.

Jade Symphony CLI remains the normal authority for Project status, claim locks,
relationships, workpads, and workflow status. Raw `gh project` or GraphQL writes
are break-glass repairs only when the CLI lacks a surface for the exact repair;
the triage note must record why the break-glass path was used.

## Required Evidence Reads

For all Doctor sessions:

```bash
cargo run -- project state workflows/jade-symphony.md
cargo run -- doctor workflows/jade-symphony.md
cargo run -- debug workflows/jade-symphony.md
```

For a selected issue:

```bash
cargo run -- project issue workflows/jade-symphony.md '#258' --json
cargo run -- doctor workflows/jade-symphony.md repair '#258'
```

For an issue that might still be dispatchable:

```bash
cargo run -- project inspect workflows/jade-symphony.md '#258'
```

For worktree or session ambiguity:

```bash
cargo run -- workspace show workflows/jade-symphony.md '#258'
cargo run -- session list workflows/jade-symphony.md
git worktree list --porcelain
```

For ordinary PR or issue text:

```bash
gh issue view 258 --repo Alive24/jade-symphony
gh pr view 123 --repo Alive24/jade-symphony --json number,isDraft,url,state
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
| `skill_install_symptom` | Local skill alias, path, metadata, or discoverability is broken. | Diagnose only; use `doctor` install-health warnings and route writes to #242. |
| `installable_suite_followup` | Packaging as a dated installable skill suite is relevant. | Route implementation to #242. |
| `issue_contract_gap` | The issue contract lacks execution-critical scope, verification, or dependency facts. | Move or recommend moving to `Need to Clarify`. |
| `no_repair_needed` | Evidence shows the item is healthy. | Return to the normal lane flow. |

Secondary classifications are useful only when they change the repair prompt or
lane handoff.

## Doctor Triage Note

Use this format for durable append-only issue timeline evidence. Write
operator-authored notes with `project timeline-comment`; do not use
`project workpad`, which is reserved for the persistent Main Agent Workpad.

```markdown
## Jade Symphony Doctor Triage

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
  - `cargo run -- project state workflows/jade-symphony.md`: `trusted=true`
  - `cargo run -- project issue workflows/jade-symphony.md '#258' --json`: ...
  - `cargo run -- doctor workflows/jade-symphony.md repair '#258'`: ...
- Recommended next step: ...
- Repair actions requiring explicit confirmation:
  - ...
- Safe no-write commands to run next:
  - ...
- Related follow-ups:
  - `doctor` covers full local skill install integrity checks and those findings are related but non-blocking.
  - #242 covers dated installable skill suite packaging and is related but non-blocking.
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
- local skill install writes.

Prefer the smallest repair that preserves issue, PR, worktree, session, and
workpad context. Do not reset when repair can preserve evidence.

## Relationship To #256 And #242

`doctor` verifies local Jade Symphony skill install health by reporting
warning-level Codex and Gemini root findings for aliases, symlinks, missing
files, stale metadata, and stale naming. Doctor triage may classify a symptom as
`skill_install_symptom` and collect evidence, but it must not repair local skill
files.

#242 is the follow-up for packaging Jade Symphony as a dated installable skill
suite. Doctor v1 may point to that follow-up when packaging is the next step,
but it must not implement the suite packaging or release layout.

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
