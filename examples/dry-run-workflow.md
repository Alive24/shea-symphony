---
tracker:
  kind: github_project_v2
  owner: Alive24
  repo: jade-symphony
  project_owner: Alive24
  project_number: 1
  status_field: Status
  fixture_path: fixtures/dry-run-issues.json
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
  root: /tmp/jade-symphony-dry-run-workspaces
agent:
  backend: dry-run
  max_concurrent_agents: 2
  max_turns: 3
  max_retry_backoff_ms: 300000
  max_concurrent_agents_by_state:
    Todo: 2
    Rework: 1
codex:
  command: codex app-server
claude:
  command: claude
review:
  backend: fake
  gemini_command: gemini
  timeout_ms: 600000
observability:
  dashboard_enabled: true
  refresh_ms: 1000
  render_interval_ms: 16
---

You are working on Jade Symphony issue {{ issue.identifier }}.

Title: {{ issue.title }}
State: {{ issue.state }}

{% if attempt %}
This is retry attempt {{ attempt }}. Resume from current workspace state.
{% endif %}

Use the issue contract, preserve tracker/backend abstractions, and keep live
GitHub Project v2 and live agent execution disabled in this dry-run workflow.
