---
tracker:
  kind: github_project_v2
  owner: Alive24
  repo: jade-symphony
  project_owner: Alive24
  project_number: 9
  status_field: Status
  fixture_path: fixtures/dogfood-smoke-issues.json
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
  active_states:
    - Todo
    - Rework
  terminal_states:
    - Done
    - Closed
    - Cancelled
    - Canceled
    - Duplicate
  assignee_filter:
    source: issue_assignees
    allow_unassigned: true
    assignees: []
  workpad:
    source: issue_comment
    marker: "<!-- jade-symphony-workpad -->"
polling:
  interval_ms: 5000
workspace:
  root: /tmp/jade-symphony-dogfood-smoke-workspaces
agent:
  backend: dry-run
  max_concurrent_agents: 1
  max_turns: 1
  max_retry_backoff_ms: 300000
codex:
  command: codex app-server
claude:
  command: claude
review:
  backend: fake
  gemini_command: gemini
  timeout_ms: 600000
observability:
  logs_root: /tmp/jade-symphony-dogfood-smoke-logs
---

You are running the fixture-backed controlled dogfood smoke workflow for
{{ issue.identifier }}.

Keep this workflow credential-free. It exists to exercise controlled smoke
candidate filtering, Issue Quality Gate evaluation, and operator reporting
without live GitHub Project v2 writes.
