---
tracker:
  kind: github_project_v2
  owner: Alive24
  repo: jade-symphony
  project_owner: Alive24
  project_number: 1
  status_field: Status
  fixture_path: fixtures/promote-issues.json
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
observability:
  logs_root: /tmp/jade-symphony-promote-fixture-logs
---

Fixture workflow for dry-run Issue Forge promotion coverage.
