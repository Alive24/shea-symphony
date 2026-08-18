# Operational Triage Reference

Read this reference only for a concrete issue, PR, claim, worktree, runtime, or
lane-handoff symptom. Repository contract repair uses
`repository-contract-repair.md` instead.

## Read boundary

Resolve the active workflow and selected capability adapter first. Prefer
targeted issue, evidence, PR, relationship, workspace, and session reads. Use a
Project-wide audit only when the selected symptom cannot be classified without
it. Raw GitHub reads are diagnostic fallbacks when the adapter lacks the exact
surface; record that gap whenever fallback evidence changes the recommendation.

Separate direct observations from Doctor inference. Preserve stale or
conflicting evidence as uncertainty rather than selecting a winner from age or
confidence of prose.

## Primary classifications

Choose one primary classification. Add a secondary classification only when it
changes the repair or handoff.

| Classification | Meaning | Normal next boundary |
| --- | --- | --- |
| `need_human_decision` | Continuation needs a credential, destructive approval, missing sample, or unstated product choice. | Ask one concrete question and preserve `Need Human Input` evidence. |
| `missing_pr_linkage` | A PR exists or is required, but targeted readback does not prove linkage. | Propose a guarded linkage repair with PR evidence. |
| `draft_pr_handoff` | The linked PR is still draft before Agent Review handoff. | Propose the supported ready repair after confirmation. |
| `stale_lane_claim` | A claim is stale, mismatched, failed, superseded, or missing registry evidence. | Preserve the claim and confirm before superseding it. |
| `dirty_runtime_or_worktree` | Runtime, session, or worktree evidence is dirty, ambiguous, or mismatched. | Preserve evidence; do not clean or relaunch speculatively. |
| `skill_loading_symptom` | A vendored Skill path, frontmatter, metadata file, or referenced resource is concretely broken. | Propose a repository-owned targeted repair without upstream comparison. |
| `issue_contract_gap` | Execution-critical scope, verification, or dependency facts are missing. | Route to Issue Forge / `Need to Clarify`; Doctor does not rewrite the issue as a shortcut. |
| `no_repair_needed` | Targeted evidence satisfies the invariant. | Return to the owning lane. |

## Repair and evidence gate

Before writing, name the violated invariant, exact target, allowed mutation,
durable evidence, readback, and refusal boundary. Require explicit confirmation
or a documented guarded write path for status changes, claim repair, PR
linkage, marking a PR ready, runtime cleanup, worktree cleanup, or vendored
Skill changes. Evidence is written before Project state; Project state is the
final mutation.

Use `.shea/template/evidence/doctor-triage.md` for append-only triage evidence.
Never overwrite the Main workpad or fabricate Review/Human evidence.

Doctor may continue into a same-session bounded repair only when the target is
known, the owning lane authority remains valid, the exact mutation is
confirmed, and durable readback can be recorded. Normal implementation,
independent Review, Human approval, and Merge return to their owning Skills.

## Outcomes

End with exactly one routing outcome:

- `resume_main`
- `resume_review`
- `resume_merge`
- `operator_confirmation_needed`
- `need_to_clarify`
- `need_human_input`
- `no_action`

Doctor never moves an issue to Human Review and never merges a PR.
