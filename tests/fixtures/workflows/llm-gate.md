---
tracker:
  kind: github_project_v2
  owner: Alive24
  repo: shea-symphony
  project_owner: Alive24
  project_number: 1
  status_field: Status
  fixture_path: ../tracker/llm-gate.json
  state_map:
    backlog: Backlog
    todo: Todo
    need_to_clarify: Need to Clarify
    in_progress: In Progress
    need_human_input: Need Human Input
    agent_review: Agent Review
    human_review: Human Review
    rework: Rework
    merging: Merging
    done: Done
quality_gate:
  llm:
    mode: required
    command: sh tests/fixtures/quality-gate/ready.sh
    timeout_ms: 5000
main_lane:
  backend: dry-run
---

You are checking issue {{ issue.identifier }} with the fixture LLM gate.
