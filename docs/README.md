# Coding Agent Context Router

Status: Accepted

Authority: Repository context routing

Scope: Select the smallest current Shea Symphony context for implementation,
review, and documentation reconciliation.

`docs/` is maintained for coding agents. It is not a second public documentation
site or a collection that should be loaded wholesale. Public explanation and
navigation are derived into `openwiki/`.

## Authority order

Use the narrowest source that owns the fact being changed:

1. Source code, tests, and configuration provide evidence of current behavior.
2. `.shea/contracts/`, repository Skills, prompts, and templates own explicit
   workflow and agent behavior contracts.
3. Accepted ADRs own approved design intent.
4. Milestone and implementation-package documents own planned boundaries and
   acceptance contracts, not live execution progress.
5. A milestone `STATUS.md` is a dated, commit-bound progress snapshot derived
   from code, tests, and the live tracker.
6. `openwiki/` is derived public synthesis and navigation. It is never an
   implementation authority.

When sources conflict, do not select a winner from file age or prose confidence.
Record the claims and ask for an explicit decision, or defer the conflict.

## Context routes

| Task | Start here | Add only when needed |
| --- | --- | --- |
| Repository onboarding | `.agents/skills/setup-shea/SKILL.md` | setup references, `.shea/workflows/shea-symphony.md`, runtime profiles |
| Shea operator workflow | the selected repository Skill | `.shea/contracts/workflow-capability.v1.md` and its selected adapter |
| Temporal / 2607 runtime | `docs/milestones/2607-hardening/README.md` | relevant ADR, package document, and `STATUS.md` |
| Tracker access and mutation | `docs/github-access-policy.md` | workflow config and tracker implementation/tests |
| Codex or Claude transport | the matching transport reference | backend implementation and focused tests |
| Prompt and template behavior | `docs/prompt-template-contract.md` | active prompt/template sources and renderer tests |
| Runtime readiness | `docs/runtime-profiles.md` | setup readiness references and active workflow config |
| App / runtime roles | `app/README.md` | `docs/legacy-runtime-distribution.md` and current Tauri code |
| Parent/subissue behavior | `docs/parent-subissue-topology.md` | optional resource contract and focused tests |
| Public explanation | `openwiki/index.md` | return to the exact repository source before changing behavior |

Issue contracts should cite exact relevant files. Coding agents should not scan
all of `docs/`, `openwiki/`, milestone notes, or Git history by default.

## Document status

- `Draft`: design or context is still under discussion.
- `Accepted`: design or context is approved for current work.
- `Superseded`: another named source replaces this document.
- `Rejected`: the proposal is intentionally not part of the design.

`Status` describes document or decision maturity. It does not describe live
implementation progress.

Every retained context document should identify one bounded subject. Add an
`Authority` or `Scope` line when ownership is not obvious. Use `Supersedes` and
`Last reconciled` only when they carry real information.

## Milestone progress

GitHub Project and Issues are the live execution authority. Each active
milestone may maintain one `STATUS.md` containing:

- the source revision and reconciliation date;
- design maturity per package;
- conservative implementation coverage;
- exact evidence and the next boundary.

The snapshot must not imply live freshness after its recorded revision. ADR
indexes track decision maturity only. Package documents hold design and
acceptance contracts rather than manually copied tracker state.

## Retention and deletion

Keep a document only when it contains a unique current decision, contract, or
agent context with an identified consumer. Extract any still-valid unique fact
to its owning Skill, contract, ADR, or technical reference, then delete the
duplicate or obsolete document. Git history retains historical material; do not
create a repository documentation archive.

Operational procedures belong in layered Skills, shared mutation semantics in
`.shea/contracts/`, and exact command syntax in the selected adapter or command
help. Do not maintain a second Markdown runbook.

## OpenWiki boundary

Update authoritative hand-written sources first. Refresh OpenWiki afterward and
cross-check its diff against code, tests, contracts, decisions, and the recorded
documentation choices. A stale or conflicting OpenWiki page must identify the
gap rather than silently resolve it.
