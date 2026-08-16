---
name: shea-symphony-improve
description: Produce a bounded, evidence-backed visual report of architectural deepening opportunities in an operator-selected repository area, or recent-change hot spots when no area is supplied. Use only when an operator explicitly asks to improve architecture, find deeper modules, reduce cross-file change friction, or assess test seams; report without changing code, docs, tracker state, or issue inventory.
---

# Shea Symphony Improve

Find at most three high-leverage architectural deepening candidates, recommend
at most one, and stop. A defensible no-finding result is successful.

## Guardrails

- Remain report-only except for the ignored local report directory and its
  confirmation-gated retention cleanup.
- Do not modify source, tests, documentation, ADRs, Project state, issues,
  workflows, or vendored Skills.
- Do not broaden into readability, lint, dependency, documentation, security,
  performance, or generic code-quality audit work.
- Do not require `CONTEXT.md`, add a configuration system, create a separate
  design Skill, grill the operator, design an interface, or implement a finding.
- Do not load prior Improve report contents unless the operator names one.

## Run the bounded phases

1. Read [scope-and-evidence.md](references/scope-and-evidence.md). Bind the
   operator's area before scanning; only infer recent-change hot spots when no
   area was supplied.
2. Read [architecture-lens.md](references/architecture-lens.md). Explore
   comprehension and change friction organically, apply the deletion and real-
   variation tests, and reject speculative seams.
3. Read [report-and-retention.md](references/report-and-retention.md). Write one
   self-contained, marker-bearing HTML report under the ignored
   `.shea/local/improve/<run-id>/` boundary and verify its limits.
4. Present the report and ask the operator to ignore a candidate, capture it
   through `$shea-symphony-backlog`, or discuss it through
   `$shea-symphony-issue-forge`.

Stop after that routing choice. Do not automatically enter design or execution.
