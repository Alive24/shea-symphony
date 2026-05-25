# Official Reference Index

This file records the official OpenAI Symphony reference material included as a
submodule for source-faithful bootstrap work.

## Upstream

- Repository: `https://github.com/openai/symphony`
- Local submodule path: `docs/bootstrap/references/openai-symphony`
- Pinned commit: `58cf97da06d556c019ccea20c67f4f77da124bf3`
- License file: `docs/bootstrap/references/openai-symphony/LICENSE`
- Notice file: `docs/bootstrap/references/openai-symphony/NOTICE`

## Normative Specification

- `docs/bootstrap/references/openai-symphony/SPEC.md`

Use this as the protocol baseline. If Shea Symphony diverges, document the
divergence explicitly in Shea Symphony-specific docs.

## Official Reference Workflow

- `docs/bootstrap/references/openai-symphony/elixir/WORKFLOW.md`

Use this as the source-faithful workflow reference. Shea Symphony-specific workflow
changes belong in `docs/bootstrap/SHEA_WORKFLOW.md`, not inside the upstream
file.

## Official Reference Implementation

- `docs/bootstrap/references/openai-symphony/elixir/README.md`
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir.ex`
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/orchestrator.ex`
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/tracker.ex`
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/linear/adapter.ex`
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/workflow.ex`
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/workflow_store.ex`
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/workspace.ex`
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/agent_runner.ex`
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/codex/app_server.ex`
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/prompt_builder.ex`
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/config/schema.ex`
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/status_dashboard.ex`
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/log_file.ex`
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/path_safety.ex`

## Official Docs Worth Reading

- `docs/bootstrap/references/openai-symphony/elixir/docs/logging.md`
- `docs/bootstrap/references/openai-symphony/elixir/docs/token_accounting.md`

## Bootstrap Rule

When implementing Shea Symphony, cite the official file path being followed in
the local implementation notes or PR summary. Do not paraphrase official
behavior from memory when the source file is available in this submodule.

