---
tracker:
  kind: linear
  endpoint: https://api.linear.app/graphql
  project_slug: jade-symphony
  fixture_path: fixtures/linear-issues.json
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
    - Canceled
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
  root: /tmp/jade-symphony-linear-fixture-workspaces
main_lane:
  backend: dry-run
  max_concurrent_agents: 1
  max_turns: 3
  max_retry_backoff_ms: 300000
codex:
  command: codex app-server -c 'service_tier="fast"'
claude:
  command: claude
observability:
  logs_root: log
---

You are working on Jade Symphony Linear issue {{ issue.identifier }}.

Title: {{ issue.title }}
State: {{ issue.state }}

Use the normalized tracker model only. Do not add Linear-specific decisions to
the orchestrator.
