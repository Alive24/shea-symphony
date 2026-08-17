# Architecture Deepening Lens

Use this lens internally; do not create another Skill or force its terms over
the repository's domain language.

## Core terms

- **Module:** code with an interface and implementation at any useful scale.
- **Interface:** everything callers and tests must know, including invariants,
  ordering, errors, configuration, and observable behavior.
- **Depth:** useful behavior hidden behind a comparatively small interface.
- **Seam:** a place where behavior can vary without editing the caller.
- **Adapter:** one concrete implementation at a seam.
- **Locality:** change, knowledge, bugs, and verification concentrate together.
- **Leverage:** many callers or tests benefit from one contained behavior.

## Candidate tests

Apply every relevant test before keeping a candidate:

1. **Deletion test:** if the module disappeared, would its complexity reappear
   across callers? If complexity simply vanishes, it is likely pass-through.
2. **Interface test surface:** callers and tests should exercise the same seam.
   A proposal that requires routine tests to reach past it is not yet deep.
3. **Real variation:** one adapter is a hypothetical seam; require at least two
   evidenced behaviors or a present dependency substitution need before adding
   an abstraction seam.
4. **Dependency-aware testing:** identify which dependency or side effect makes
   verification difficult and how the deeper direction improves focused and
   integration coverage without mocking implementation details.
5. **Locality and leverage:** name the repeated knowledge that would move behind
   the interface and the callers/tests that would become simpler.

Reject candidates supported only by possible future variation, aesthetic
preference, renaming, general cleanup, or a desire for more layers. Do not
design the replacement interface; describe only the deepening direction and
the evidence a later Issue Forge discussion must resolve.

Rank retained candidates `Strong` or `Worth exploring`. Keep speculative ideas
out of the report. If none survives, report no finding and the evidence checked.
