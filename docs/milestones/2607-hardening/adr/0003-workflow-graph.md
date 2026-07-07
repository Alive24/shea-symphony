# ADR 0003: Workflow Graph

Status: Proposed

## Context

Hook-style extension points are easy to add but hard to visualize, resume, or
compose. A persistent graph better matches the workflow: nodes represent stages
and extension nodes, while edges represent transition conditions.

## Decision

Represent Workflow Graph as the long-term direction, not a full 2607 runtime
replacement. Defer implementation to 2608 Workflow Graph Extension.

During hardening, organize the existing workflow around Tracker State, standard
Symphony behavior, and explicit extension insertion points. Existing hooks may
remain when they are attached to a clear state or standard node.

In 2608, support standard nodes implemented by Symphony and extension nodes
configured by the workflow. Standard nodes are not replaced in place; they may
be disabled or bypassed by graph configuration.

Extension nodes may recommend graph edges or entry into a standard core node.
Tracker state changes still go through the Symphony transition path.

Use fixed enum edge conditions when graph execution is introduced.

## Consequences

- 2607 can prepare the shape without migrating the full runtime.
- Consecutive extension nodes should become first-class graph nodes in 2608.
- Extension nodes can influence workflow direction without owning tracker
  writes.
- App visualization can start with state-grouped workflow structure.
- Resume behavior should remain state-based in 2607 and later incorporate graph
  position.

## Follow-Up

- Define compatibility rules for current workflow config.
- Define state-grouped workflow structure.
- Define extension module insertion semantics in 2608.
