# Shea Symphony Bootstrap

This directory contains the bootstrap materials for building Shea Symphony, a
private-first team harness for orchestrating coding agents.

## Source Boundaries

- Official OpenAI Symphony material lives under
  `docs/bootstrap/references/openai-symphony` as a Git submodule.
- Do not edit the official submodule directly.
- Shea Symphony-specific interpretation and implementation decisions live in this
  directory.
- Future implementation code should live outside `docs/bootstrap`.

## Required Reading Order

1. `docs/bootstrap/references/openai-symphony/SPEC.md`
2. `docs/bootstrap/references/openai-symphony/elixir/WORKFLOW.md`
3. `docs/bootstrap/references/openai-symphony/elixir/README.md`
4. `docs/bootstrap/OFFICIAL_REFERENCE_INDEX.md`
5. `docs/bootstrap/SHEA_SYMPHONY_SPEC.md`
6. `docs/bootstrap/TRACKER_GITHUB_PROJECT_V2.md`
7. `docs/bootstrap/SHEA_WORKFLOW.md`
8. `docs/bootstrap/ISSUE_QUALITY_GATE_TEMPLATE.md`

## Implementation Posture

Shea Symphony should be built from the official Symphony specification, not by
blindly porting the Elixir reference implementation. The Elixir code is a
reference for behavior, structure, and operational tradeoffs.

The first implementation target is a Rust CLI with GitHub Project v2 as the
first tracker adapter and Linear kept as a required future adapter.
