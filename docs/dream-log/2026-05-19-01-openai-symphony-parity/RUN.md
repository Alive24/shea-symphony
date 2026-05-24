# Dream Run: OpenAI Symphony Parity

Date: 2026-05-19
Run: `2026-05-19-01-openai-symphony-parity`
Mode: write-mode Dream continuation
Operator context: continue sleeping on Shea Symphony against the OpenAI Symphony specification and Elixir reference implementation

## Source Inventory

- `git status --short --branch`
- `git fetch origin`
- `git merge --ff-only origin/main`
- `cargo run -- project state workflows/shea-symphony.md`
- `cargo run -- doctor workflows/shea-symphony.md`
- `cargo run -- debug workflows/shea-symphony.md`
- `gh issue list --repo Alive24/shea-symphony --state open --search "Backlog: in:title" --json number,title,url --limit 80`
- `docs/dream-log/INDEX.md`
- `docs/dream-log/2026-05-18-02-dream-cli-skill-drift/RUN.md`
- `docs/dream-log/2026-05-18-02-dream-cli-skill-drift/topic-cli-skill-drift.md`
- `docs/bootstrap/references/openai-symphony/SPEC.md`
- `docs/bootstrap/references/openai-symphony/elixir/README.md`
- `docs/bootstrap/references/openai-symphony/elixir/WORKFLOW.md`
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir.ex`
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/orchestrator.ex`
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/workflow_store.ex`
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/codex/dynamic_tool.ex`
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir_web/controllers/observability_api_controller.ex`
- `docs/bootstrap/references/openai-symphony/elixir/docs/logging.md`
- `docs/bootstrap/references/openai-symphony/elixir/docs/token_accounting.md`
- `docs/implementation_notes.md`
- `docs/dogfood-readiness.md`

## Round Summary

This round compared the current Shea Symphony implementation notes and dogfood
readiness surface against the OpenAI Symphony spec plus the Elixir reference.
The strongest gaps were not small doc drift; they were runtime parity boundaries
that appear in the reference as first-class orchestration behavior:

- Codex app-server continuation, max-turn accounting, token/rate-limit handling,
  and live runtime snapshot wiring.
- Tracker-scoped dynamic tool execution, especially the `linear_graphql` shape
  in the reference implementation, while Shea currently has descriptor-only
  dynamic-tool planning.
- Last-known-good workflow reload semantics wired into long-running runtimes
  instead of only one-shot load/reload helpers.

The optional observability API/web dashboard stayed on the Watchlist. Shea has a
deliberate terminal-first status path, `status serve --once`, JSONL events, and
runtime snapshots; `docs/implementation_notes.md` marks the persistent API layer
as delayed rather than merely forgotten.

## Created Backlog

- #321 `Backlog: shape Codex app-server continuation parity`
- #322 `Backlog: define tracker-scoped dynamic tool execution`
- #323 `Backlog: wire last-known-good workflow reload into runtimes`

## Watchlist / Not Created

- Persistent observability API/dashboard: not created because the current repo
  notes explicitly delay the HTTP/API layer, and existing status/runtime work
  already covers the terminal-first operator surface. This may deserve a later
  seed only after the worker runtime snapshot is wired to live orchestration.
- Retry timers, stall detection, and worker supervision: not created in this
  round because the parity table already calls it out, and nearby retry/stall
  Backlog/Todo issues (#305, #312, #318) cover current dogfood pain. It remains
  the best next theme for a deeper Dream pass.
- Local skill install drift: not created because #242, #256, and #315 already
  own install/update/readiness surfaces.
- Repo-owned skill command drift: not created because #320 already exists.
- Codex tmux config-migration prompts: not created because #319 already exists.

## Doctor / Project Warnings

- `project state` after creation reports `Agent Review:1, Backlog:13, Done:77,
  Todo:4`, with the canonical checkout clean and `origin/main` already merged.
- `doctor` remains warning-only with no blockers.
- The prominent Project warning is #243
  `terminal_lane_claim_missing_registry` for a terminal Review Agent claim with
  no matching runtime/session registry evidence.
- Local Codex and Gemini skill install warnings remain present; they should
  probably route through the #242/#256/#315 family instead of a new Dream seed.
- Integration gaps remain: Project v2 PR linking is still comment/autolink
  based, and live Project writes still go through `gh api graphql` under Shea
  Symphony CLI `--write` commands.

## Gemini Review Status

Gemini review passed. See `gemini-review.md`.

## Slept Enough

Slept enough: no.

Reason: this round created the strongest OpenAI Symphony parity seeds, but the
reference comparison still has one high-signal unexplored theme: retry timers,
stall detection, worker supervision, and continuation scheduling. That theme
should be checked carefully against existing #305/#312/#318 coverage before any
new seed is created.

## Safety Notes

Dream-created issues stayed in `Backlog`. None were promoted to Todo, claimed by
Main/Review/Merge, or treated as lane authority. Project reads and issue
creation went through the Shea Symphony CLI where Project state or mutation was
involved.
