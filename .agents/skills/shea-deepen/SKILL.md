---
name: shea-deepen
description: Produce a bounded, report-only, evidence-backed visual assessment of architectural deepening opportunities in an operator-selected code, runtime, workflow-contract, or test area, or recent-change hot spots when no area is supplied. Use when the primary request is to reduce cross-file change friction, deepen modules, localize behavior, or assess test seams. Do not use for documentation correctness, freshness, reconciliation, or OpenWiki work; concrete failure, stuck-execution, or faulty-configuration diagnosis or repair; or implementation.
---

# Shea Deepen

Find at most three high-leverage architectural deepening candidates, recommend
at most one, and stop. A defensible no-finding result is successful.

## Route by primary object

Before scanning, classify the request by its primary object, not its file type.
Continue for architecture and change-locality assessment, including structural
questions about behavior-bearing Markdown. Route a concrete failure or repair
to `$shea-doctor`. For documentation correctness, freshness, reconciliation,
or OpenWiki work, route to `$shea-docs` when installed; otherwise stop without
using Deepen or Doctor as a fallback. Documentation may still serve as evidence
after an architecture scope is bound.

## Guardrails

- Remain report-only except for the ignored local report directory and its
  confirmation-gated retention cleanup.
- Do not modify source, tests, documentation, ADRs, Project state, issues,
  workflows, or vendored Skills.
- Do not broaden into readability, lint, dependency, documentation, security,
  performance, or generic code-quality audit work.
- Do not require `CONTEXT.md`, add a configuration system, create a separate
  design Skill, grill the operator, design an interface, or implement a finding.
- Do not load prior Deepen report contents unless the operator names one.

## Run the bounded phases

1. Read [scope-and-evidence.md](references/scope-and-evidence.md). Bind the
   operator's area before scanning; only infer recent-change hot spots when no
   area was supplied.
2. Read [architecture-lens.md](references/architecture-lens.md). Explore
   comprehension and change friction organically, apply the deletion and real-
   variation tests, and reject speculative seams.
3. Read [report-and-retention.md](references/report-and-retention.md). Write one
   self-contained, marker-bearing HTML report under the ignored
   `.shea/local/deepen/<run-id>/` boundary and verify its limits.
4. Present the report and ask the operator to ignore a candidate, capture it
   through `$shea-backlog`, or discuss it through
   `$shea-issue-forge`.

Stop after that routing choice. Do not automatically enter design or execution.
