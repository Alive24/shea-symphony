---
kind: shea-workflow-capability-adapter
adapter_id: legacy-cli-v1
adapter_version: 1
capability: ../workflow-capability.v1.md
runtime_role: legacy_cli
compatibility: shea-legacy-cli-v1
---

# Legacy CLI Adapter

This adapter maps Workflow Capability v1 semantics to the current compatibility
CLI and narrowly permitted GitHub content reads. The stable contract owns
ordering, confirmation, uncertainty, and authority; this file owns only Legacy
surface selection and syntax.

Resolve the CLI and workflow paths from the active repository's app profile,
then repository-local defaults. Verify the selected executable's runtime role
and compatibility before use. Never embed a resolved machine path here.

In the mappings below, `CLI`, `WORKFLOW`, `ISSUE`, `PR`, and Markdown paths are
resolved inputs, not committed values.

## Targeted Read Mappings

| Semantic name | Legacy surface |
| --- | --- |
| `workflow.resolve` | Resolve the app profile, then validate `CLI WORKFLOW --help` and the workflow. |
| `issue.read` | `CLI project issue WORKFLOW ISSUE --json` |
| `issue.inspect` | `CLI project inspect WORKFLOW ISSUE --lane LANE` |
| `evidence.read` | `CLI project issue WORKFLOW ISSUE --json`; use its canonical workpad and timeline evidence. |
| `pull_request.read` | Use `issue.read` for exact linked-PR source and a targeted `gh pr view PR` only for raw PR content/readiness absent from normalized issue output. |
| `relationships.read` | `CLI project relationship list WORKFLOW ISSUE` |

Raw issue or PR reads do not replace normalized Project state, relationship,
claim, workpad, or linked-PR readback.

## Guarded Action Mappings

All mappings first use their dry-run or read-only preparation surface when one
exists, then add `--write` only during Execute.

| Semantic name | Legacy Execute surface | Targeted readback |
| --- | --- | --- |
| `workspace.adopt` | `CLI workspace adopt WORKFLOW ISSUE PATH --write` | `CLI workspace show WORKFLOW ISSUE` |
| `lane.claim` | `CLI LANE claim WORKFLOW ISSUE --worker WORKER --write` | `issue.read` |
| `workpad.upsert` | `CLI project workpad WORKFLOW ISSUE MARKDOWN --write` | `evidence.read` |
| `timeline.append` | `CLI project timeline-comment WORKFLOW ISSUE MARKDOWN --write` | `evidence.read` |
| `issue.transition` | `CLI project set-state WORKFLOW ISSUE STATE --write` | `issue.read` |
| `pull_request.link` | `CLI project link-pr WORKFLOW ISSUE PR --write` | `issue.read` and preserve the exact reported link source |
| `relationship.add_blocked_by` | `CLI project relationship add-blocked-by WORKFLOW ISSUE BLOCKER --write` | `relationships.read` |
| `relationship.add_subissue` | `CLI project relationship add-subissue WORKFLOW PARENT CHILD --write` | `relationships.read` for both issues |
| `issue.create` | `CLI forge create --workflow WORKFLOW` with the prepared contract and `--write` | `issue.read` for the returned issue |
| `issue.promote` | `CLI forge promote ISSUE --workflow WORKFLOW` with the prepared contract, confirmation evidence, and `--write` | `issue.read` |
| `issue.rework` | `CLI forge rework ISSUE --workflow WORKFLOW` with the prepared contract, review evidence, confirmation, and `--write` | `issue.read` |

The Legacy adapter additionally supports `CLI forge validate` as the read-only
quality preparation surface for create, promote, and rework decisions.

## Legacy Failure Mapping

- Nonzero exit before a mutation request is sent maps to `not_applied` when the
  diagnostic explicitly proves no tracker mutation occurred.
- A validation, eligibility, authority, or canonical-checkout refusal maps to
  `rejected` with the emitted reason.
- Network failure, process interruption, recovery marker, or missing response
  after a write attempt maps to `uncertain`.
- On `uncertain`, invoke only the mapped targeted readback. Do not repeat a
  relationship addition, comment append, claim, state transition, or Forge
  mutation until readback proves `not_applied` and preparation still matches.
- Preserve recovery markers and exact linked-PR source values as evidence; do
  not upgrade diagnostic fallback evidence to native linkage.
