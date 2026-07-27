A code wiki for Shea Symphony. Prioritize a concise quickstart, architecture overview, source map, key workflows, domain concepts, operations and runbook guidance, testing, and integration points.

Explain the Temporal-based orchestration model, lifecycle and reconciliation boundaries, operator workflows, Doctor, Human Review, NHI, NTC, and the relationships between tracker state, runtime authority, and local projections.

Treat the generated OpenWiki as a derived navigation and synthesis layer, not an authoritative replacement for source code, existing repository documentation, or the live GitHub tracker. Preserve explicit document status such as Draft, Proposed, and Accepted. Clearly distinguish implemented behavior from planned design, and surface conflicts or drift instead of resolving them silently.

Prioritize the current 2607 hardening work and explain its relationship to the 2606 MVP without conflating the two. Summarize and link useful existing documentation rather than duplicating it wholesale. Do not infer live issue or project progress from repository documents.

Under .shea, inspect only .shea/prompts/**, .shea/template/**, and .shea/workflows/** when they help explain agent or operator workflows. Do not inspect, search, index, or document .shea/artifacts/**, .shea/logs/**, .shea/worktrees/**, .shea/local/**, .shea/**/*.local.*, .shea/app/**, or .shea/bin/**.

Deprioritize docs/dream-log, docs/assets, vendored bootstrap references, generated artifacts, build outputs, and local logs unless they are directly needed to explain current behavior.

Inspect recent git history when it helps explain why important behavior exists. Keep pages grounded in repository evidence and prefer practical navigation for engineers and future agents over generic summaries.
