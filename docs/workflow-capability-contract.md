# Workflow Capability Contract

The repository-owned Workflow Capability contract lives at
`.shea/contracts/workflow-capability.v1.md`. It gives Shea skills stable names
for targeted reads and guarded actions plus one prepare, confirm, execute, and
readback protocol. Concrete runtime syntax lives in a separately versioned
adapter under `.shea/contracts/adapters/`.

This is the authoritative skill-suite ownership note for that shared contract;
skill installation and distribution remain documented separately.

## Ownership Boundary

- Workflow configuration owns target repository, tracker, state mapping,
  workspace, lane backend, and verification policy.
- Machine-local profiles own resolved executables and environment requirements.
- The Workflow Capability contract owns stable semantic names and mutation
  safety rules without copying those values.
- Adapters own runtime-specific mappings without redefining stable semantics.
- Skills own conversational and lane authority and declare only the semantic
  subset they consume.

The contract is not a second workflow file, an action registry, or a permission
grant. Consumers must resolve its active workflow and adapter references, honor
their own narrower authority, and fail closed when references or readback are
uncertain. Representative Manual Main and Reflect fixtures under
`tests/fixtures/workflow-capability/` demonstrate consumption without changing
the live skills.
