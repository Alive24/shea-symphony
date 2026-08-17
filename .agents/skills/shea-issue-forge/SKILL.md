---
name: shea-issue-forge
description: Shape rough operator intent into a quality-gated Shea issue through focused discussion, explicit confirmation, and guarded creation, promotion, or rework.
---

# Shea Issue Forge

Turn rough operator intent into one executable issue contract. Conversation and
drafting happen here; deterministic validation and tracker writes happen only
through the active workflow capability and its selected adapter.

## Route the active phase

1. Read [discussion.md](references/discussion.md) while resolving intent,
   boundaries, dependencies, and whether native parent/subissue topology is
   warranted.
2. Read [contract.md](references/contract.md) when drafting or checking the
   executable issue body and quality gate.
3. Read [tracker-hygiene.md](references/tracker-hygiene.md) before any live
   freshness claim, temporary draft write, or tracker mutation.
4. Read only the mutation reference that matches the confirmed action:
   [creation.md](references/creation.md),
   [promotion.md](references/promotion.md), or
   [rework.md](references/rework.md).

## Authority

Resolve `.shea/contracts/workflow-capability.v1.md`, its `active_workflow`, and
a supported adapter before tracker reads or actions. The capability contract
owns targeted reads, guarded-action ordering, uncertainty, and readback; the
adapter owns syntax. Fail closed when a required binding or capability is
missing.

Never bypass the quality gate, duplicate an issue when tracker reads are
unavailable, or modify implementation code. Show the complete prepared effect
and obtain explicit confirmation before a guarded mutation unless the operator
directly supplied a complete body and exact mutation instruction. Creation,
promotion, or the final Rework state transition is the phase's final mutation;
perform targeted readback only afterward.
