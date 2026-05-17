# Jade Symphony Example Workflows

This directory contains fixture, demo, and compatibility workflows for
rehearsing Jade Symphony behavior. Most examples are fixture-backed and
credential-free.

The normal repo dogfood workflow is not in this directory. Use
`workflows/jade-symphony.md` for live Project #9 operator runs. The live GitHub
Project examples remain as compatibility/reference material for debugging older
commands and testing specific lanes.

## Fixture Dispatch

| Workflow | Purpose | Safe commands |
| --- | --- | --- |
| `dry-run-workflow.md` | Main credential-free dispatch and run-loop fixture backed by `fixtures/dry-run-issues.json`. | `cargo run -- plan examples/dry-run-workflow.md`; `cargo run -- run-loop examples/dry-run-workflow.md --max-iterations 1 --dry-run` |
| `source-alignment-workflow.md` | Source-alignment gate fixture with one valid and one broken issue. | `cargo run -- plan examples/source-alignment-workflow.md`; `cargo run -- gate examples/source-alignment-workflow.md '#1'` |
| `usage-limit-workflow.md` | Fixture backend path that exercises usage-limit pause handling. | `cargo run -- run-loop examples/usage-limit-workflow.md --max-iterations 1 --write` |
| `git-identity-workflow.md` | Workspace-local git identity application fixture. | `cargo run -- run-once examples/git-identity-workflow.md` |

Fixture workflows may use `--write` when the tracker is fixture-backed or
memory-backed. They do not mutate live GitHub Project v2 state.

## Tracker Adapters

| Workflow | Purpose | Notes |
| --- | --- | --- |
| `linear-fixture-workflow.md` | Linear adapter fixture backed by `fixtures/linear-issues.json`. | Credential-free; does not prove live Linear readiness. |
| `github-project-workflow.md` | Legacy live GitHub Project v2 template for Project #9. | Compatibility/reference workflow; prefer `workflows/jade-symphony.md` for normal operator runs. |
| `github-project-gemini-review-workflow.md` | Legacy live GitHub Project v2 Review Agent template for Project #9. | Compatibility/reference workflow; `workflows/jade-symphony.md` carries the normal review config. |

## Agent Backend Fixtures

| Workflow | Purpose | Notes |
| --- | --- | --- |
| `codex-subprocess-workflow.md` | Conservative Codex subprocess backend fixture. | Runs the configured command in the prepared workspace. |
| `claude-subprocess-workflow.md` | Conservative Claude Code subprocess backend fixture. | Separate backend path from Codex; not full protocol parity. |
| `cockpit-profiles-workflow.md` | Execution profile discovery from a cockpit-tools-style fixture. | Reads `fixtures/cockpit-tools-codex-instances.json`; no secrets are used. |

## Review And Quality Gate Fixtures

| Workflow | Purpose | Notes |
| --- | --- | --- |
| `review-fixture-workflow.md` | Review-loop fixture backed by `fixtures/review-issues.json`. | Uses configured review flow without live reviewer credentials. |
| `review-fake-workflow.md` | Fake review backend smoke path over dry-run issues. | Useful for role-bound transition checks. |
| `llm-gate-workflow.md` | Optional command-backed LLM quality gate fixtures. | Uses shell fixtures in `fixtures/llm-gate-*.sh`; no hosted provider is called. |

## Topology Fixtures

| Fixture | Purpose | Verification |
| --- | --- | --- |
| `fixtures/parent-subissue-topology.json` | Dry parent/subissue integration branch topology examples for the documented `#243` parent flow. | `cargo test parent_subissue_topology` |

## Fixture Data

| Fixture | Used by |
| --- | --- |
| `fixtures/dry-run-issues.json` | `dry-run-workflow.md`, `codex-subprocess-workflow.md`, `claude-subprocess-workflow.md`, `review-fake-workflow.md`, `cockpit-profiles-workflow.md` |
| `fixtures/source-alignment-issues.json` | `source-alignment-workflow.md` |
| `fixtures/usage-limit-issues.json` | `usage-limit-workflow.md` |
| `fixtures/git-identity-issues.json` | `git-identity-workflow.md` |
| `fixtures/linear-issues.json` | `linear-fixture-workflow.md` |
| `fixtures/review-issues.json` | `review-fixture-workflow.md` |
| `fixtures/llm-gate-issues.json` | `llm-gate-workflow.md` |
| `fixtures/cockpit-tools-codex-instances.json` | `cockpit-profiles-workflow.md` |
| `fixtures/parent-subissue-topology.json` | `docs/parent-subissue-topology.md` and `tests/parent_subissue_topology.rs` |

Issue Forge examples:

- `fixtures/thin-issue.md`
- `fixtures/repaired-issue.md`

## Live Boundary

Use `../workflows/jade-symphony.md` for normal live Project #9 implementation,
review, merge, smoke, inspect, and Issue Forge commands. The legacy live
examples default to `~/.jade-symphony/artifacts` when
`JADE_SYMPHONY_ARTIFACT_ROOT` is unset, and support setting that environment
variable to move durable worktrees, logs, and review artifacts together.

Do not treat fixture success as live readiness. Before any live write, inspect
the Project state, confirm the issue contract, and use the workflow-specific
commands documented in the root README and dogfood docs. If an operator has a
workflow file under `/tmp` or `/private/tmp`, promote the reusable config or
prompt into `workflows/`, `examples/`, or `docs/` before relying on it for
dogfood. Normal operator workflow config belongs in `workflows/`.
