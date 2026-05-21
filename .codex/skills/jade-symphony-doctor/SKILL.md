---
name: jade-symphony-doctor
description: Diagnose Jade Symphony doctor findings, Need Human Input items, issue or PR blockers, and install-health gaps, then give an explicit repair recommendation and execute confirmed safe repairs in the same session when the workflow contract allows it.
metadata:
  short-description: Jade Symphony Doctor triage
---

# Jade Symphony Doctor

Use this skill when a Jade Symphony operator asks to diagnose a stuck workflow
state, a `Need Human Input` item, doctor warning, install-health finding, or a
specific issue/PR/worktree that needs a safe next step before normal Main,
Review, or Merging work can continue.

Doctor owns diagnosis, classification, evidence capture, and a concrete repair
recommendation. It may continue in the original Codex session to execute a
confirmed repair when the required lane, CLI command, and workflow evidence are
clear. It does not replace normal implementation, independent review approval,
human approval, or merge authority.

## Repository

Default repository:

```bash
cd /Volumes/Bohemialive/GitHub/jade-symphony
```

Canonical workflow:

```bash
workflows/jade-symphony.md
```

Supporting operator spec:

```bash
docs/operator-doctor.md
```

## Invocation

Use this skill for:

- `Need Human Input` issues where the next safe repair step is unclear.
- operator-selected issue refs, such as `#258`, `issue 258`, or a PR linked to
  a workflow item.
- PR linkage gaps before Main, Review, or Merge handoff.
- stale or ambiguous lane claim fields.
- dirty runtime state, session registry, or issue worktree symptoms.
- local skill install symptoms and installable skill suite drift.

Do not use this skill to claim new `Todo` implementation work. Use
`$jade-symphony-manual-main` for Main Agent implementation and
`$jade-symphony-manual-merge` for merge-lane repair.

## Authority

Jade Symphony CLI is the authority for Project state, claim locks,
relationships, workpads, and workflow status.

Start read-only. Do not mutate Project status, claim fields, PR readiness,
worktrees, local skills, or runtime artifacts until one of these is true:

- the operator explicitly asked for that repair in the current session;
- the operator confirms the specific proposed repair after diagnosis;
- the repair is a documented Jade Symphony CLI path whose `--write` behavior is
  already required by the active workflow step.

Break-glass raw `gh project` or GraphQL mutations are allowed only when the CLI
lacks the needed repair surface. Record the reason, exact command family, and
evidence preserved before the mutation.

## Required Read-Only Preflight

Run or equivalent-check these before recommending repair:

```bash
cargo run -- project state workflows/jade-symphony.md
cargo run -- doctor workflows/jade-symphony.md
cargo run -- debug workflows/jade-symphony.md
```

For an operator-selected issue:

```bash
cargo run -- project issue workflows/jade-symphony.md '#258' --json
cargo run -- doctor workflows/jade-symphony.md repair '#258'
```

If the issue appears to be implementation-claimable or contract-blocked, also
run:

```bash
cargo run -- forge validate --workflow workflows/jade-symphony.md --issue '#258'
```

If worktree or session evidence is ambiguous, add the narrowest relevant reads:

```bash
cargo run -- workspace show workflows/jade-symphony.md '#258'
cargo run -- session list workflows/jade-symphony.md
git worktree list --porcelain
```

For install-health or installable-suite findings, show the target paths before
any write:

```bash
node scripts/install-jade-symphony-skills.js --dry-run
node scripts/install-jade-symphony-skills.js --validate
```

Use `gh issue view` and `gh pr view` only for ordinary issue/PR content when
the CLI lacks the needed content read, and record the CLI gap when the result
changes the repair recommendation. Do not use raw Project reads for status,
claim fields, blockers, workpads, or linked PR state in normal flow.

## Classification

Classify the problem into one primary category. Add secondary categories only
when they change the recommended next step.

- `need_human_decision`: continuation depends on an operator/product decision,
  credentials, destructive approval, missing sample data, or a choice that the
  issue contract did not authorize.
- `missing_pr_linkage`: a lane handoff or merge needs a Project-visible linked
  PR, but `project issue` does not expose the expected PR.
- `draft_pr_handoff`: the linked PR exists but is draft before an Agent Review
  handoff.
- `stale_lane_claim`: `Main Agent`, `Review Agent`, or `Merging Agent` points to
  a stale, failed, superseded, mismatched, or registry-missing run.
- `dirty_runtime_or_worktree`: local runtime state, session registry, or issue
  worktree evidence is dirty or ambiguous enough that cleanup could discard
  useful work.
- `skill_install_symptom`: local skill alias, install path, metadata, or
  discoverability appears broken. Diagnose, show target paths, and recommend the
  exact suite install or targeted local copy repair. Execute only after explicit
  operator request or confirmation.
- `installable_suite_followup`: dated installable skill packaging is relevant
  and needs repo-owned suite changes before local install.
- `issue_contract_gap`: the issue cannot safely execute because required
  context, verification, dependencies, or scope boundaries are missing.
- `no_repair_needed`: evidence shows the item is healthy and the next step is a
  normal lane command.

## Doctor Triage Note

When the operator asks for a durable result, write or propose this note as a
GitHub issue comment through the configured timeline-comment path. Do not use
`project workpad`, which is reserved for persistent lane workpads.

```markdown
### Doctor Triage Note

- Issue: `#258`
- Status at triage: `Need Human Input`
- Primary classification: `missing_pr_linkage`
- Secondary classifications: `stale_lane_claim`
- Diagnosis: ...
- Evidence read:
  - `cargo run -- project state workflows/jade-symphony.md`: ...
  - `cargo run -- project issue workflows/jade-symphony.md '#258' --json`: ...
  - `cargo run -- doctor workflows/jade-symphony.md repair '#258'`: ...
- Recommended next step: ...
- Explicit repair recommendation: ...
- Can execute in this session: `yes` | `no`
- Repair actions requiring explicit confirmation:
  - ...
- Safe no-write commands to run next:
  - ...
- Related follow-ups:
  - #256 is related to local skill install integrity and is not implemented here.
  - #242 is related to installable skill suite packaging and is not implemented here.
```

The note must name what was read, what was inferred, and what remains unsafe to
do without explicit confirmation.

## Repair Recommendation And Execution

After diagnosis, do not stop at a vague routing label. Always provide one
explicit next action:

- a lane handoff command, such as `$jade-symphony-manual-main`,
  `$jade-symphony-manual-review`, or `$jade-symphony-manual-merge`;
- a Jade Symphony CLI repair command, such as `project set-state`,
  `project link-pr`, `doctor ... repair`, or `project timeline-comment`;
- a local install-health command, such as a suite dry-run, validate, or
  confirmed install/update path;
- one operator question when the evidence still depends on a human decision.

If the repair is confirmed and fits the workflow contract, continue in the same
Codex session instead of forcing a new session. Before executing, state:

- the target issue/PR or local skill path;
- the owning lane or installer path;
- the exact command or file copy target;
- why the prior review, human approval, or local evidence remains valid.

Switch to the owning skill or lane workflow when the repair is normal Main,
Review, Human Review, or Merging work. Doctor may coordinate the handoff and
continue the session, but it must preserve those authority boundaries.

## Repair Boundaries

Prefer repair over reset when issue, PR, worktree, or session context can be
preserved.

Allowed by default:

- read Project, issue, doctor, debug, worktree, session, and PR evidence.
- classify the stuck state.
- draft a `Doctor Triage Note`.
- recommend the smallest safe next command.
- identify which lane owns the next step.

Requires explicit operator confirmation or a documented `--write` path:

- Project status changes.
- claim field repair or superseding lane claims.
- PR linkage repair.
- marking a PR ready.
- worktree cleanup, archive, or deletion.
- runtime state cleanup.
- moving an issue to `Need Human Input`.
- local skill install or overwrite writes.

Allowed after explicit operator request or confirmation:

- executing a documented Jade Symphony CLI repair command.
- performing merge-lane-only conflict repair through the Merging workflow.
- copying or installing a repo-owned skill suite entry into the shown local
  Codex/Gemini target path.
- writing a Doctor triage or repair evidence note with
  `project timeline-comment`.

Out of scope for Doctor v1:

- unconfirmed automatic PR linkage repair.
- unconfirmed automatic stale claim repair.
- unconfirmed automatic worktree cleanup.
- unconfirmed automatic Project mutation.
- replacing `doctor`, `project state`, or `project issue`.

## Handoff

End with one concrete next step:

- `resume_main`: use Main Agent flow on an executable issue.
- `resume_review`: use Review Agent flow on an Agent Review item.
- `resume_merge`: use Merging Agent flow on a Merging item.
- `operator_confirmation_needed`: ask for the exact write/repair approval.
- `need_to_clarify`: issue contract needs repair before dispatch.
- `need_human_input`: durable evidence is preserved and human input is the next
  blocker.
- `no_action`: no stuck state remains.

Never move an issue to `Human Review`. Never merge a PR.
