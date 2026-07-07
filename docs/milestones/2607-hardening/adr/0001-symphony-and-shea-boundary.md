# ADR 0001: Symphony And Shea Boundary

Status: Proposed

## Context

The MVP mixes reliable runtime concerns with extension and operator policy.
That made dogfood possible, but it also made non-LLM paths slower and harder to
reason about.

## Decision

Use `Symphony` for the reliable workflow runtime and `Shea` for the extension
layer.

Symphony owns hard execution concerns: workflow state, tracker reads and
writes, worktrees, agent runner lifecycle, review, merge, runtime state,
logging, traceability, status snapshots, retry, stall detection, and
reconciliation.

Shea owns extension concerns: skills, prompt templates, semantic gates, Issue
Forge, Dream/Reflect style backlog mining, and operator interaction policy.

## Consequences

- Review and merge are Symphony workflow stages, not Shea-only extensions.
- Shea can propose actions, graph edges, or next core nodes, but Symphony
  applies tracker transitions.
- The App consumes Symphony state instead of becoming source of truth.

## Follow-Up

- Define 2607 workflow structure contracts and defer full Workflow Graph
  contracts to 2608.
- Define tracker write ownership.
- Define extension result schema.
