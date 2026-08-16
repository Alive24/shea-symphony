# Workflow Capability Contract

The repository-owned Workflow Capability contract lives at
`.shea/contracts/workflow-capability.v1.md`. It gives Shea skills stable names
for targeted reads and guarded actions plus one prepare, confirm, execute, and
readback protocol. Concrete runtime syntax lives in a separately versioned
adapter under `.shea/contracts/adapters/`.

This is the authoritative Skill ownership note for that shared contract.
First-party sources live under `.agents/skills`; repositories own and may
customize any copies they vendor from that tree.

## Ownership Boundary

- Workflow configuration owns target repository, tracker, state mapping,
  workspace, lane backend, verification policy, and repository Markdown paths
  for lane prompts and workpad templates.
- Machine-local profiles own resolved executables and environment requirements.
- The Workflow Capability contract owns stable semantic names and mutation
  safety rules without copying those values.
- Adapters own runtime-specific mappings without redefining stable semantics.
- Skills own conversational and lane authority and declare only the semantic
  subset they consume.
- Repository Markdown prompt/workpad files own agent- and operator-facing prose;
  Rust owns typed values, strict rendering, section-aware idempotent merge
  mechanics, fail-closed validation, and tracker transport.

The contract is not a second workflow file, an action registry, or a permission
grant. Consumers must resolve its active workflow and adapter references, honor
their own narrower authority, and fail closed when references or readback are
uncertain. Consumer fixtures under `tests/fixtures/workflow-capability/` and the
live operational Skills both enforce consumption without copying adapter
commands.

## Migration Inventory

The normal operational layer was deliberately collapsed into capability
consumers:

| Surface | Before | After | Authority now |
| --- | ---: | ---: | --- |
| Manual Main, Manual Review, Manual Merge, Human Review, Doctor Skills | 1,072 lines | 185 lines | Lane/operator policy plus Workflow Capability references |
| Main, Review, Merge lane prompts | 242 lines | 73 lines | Concise lane authority and completion protocol |
| Runtime workpad prose | Scattered Rust builders and partial Markdown fallbacks | Repository Markdown registry | `.shea/template/workpad/*.md` |

The canonical workflow must name every required workpad template. Validation
fails closed for a missing map entry or missing, empty, unreadable, or malformed
template. A repository Markdown default exists only for code paths without an
active workflow; an active configured workflow never falls back to Rust prose.

Canonical Main workpad writes are section-aware. Updating Run Identity,
Verification, PR / Linkage, Recovery / Rework, or Handoff replaces that stable
section in place, preserves unrelated stable sections, and collapses duplicate
canonical top-level workpads. Review, Human Review, Doctor, and Merge records
remain append-only.
