---
tracker:
  kind: github_project_v2
  owner: Alive24
  repo: jade-symphony
  project_owner: Alive24
  project_number: 9
  status_field: Status
  fixture_path: fixtures/merge-issues.json
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
workspace:
  root: /tmp/jade-symphony-merge-fixture-workspaces
agent:
  backend: dry-run
  max_concurrent_agents: 1
  max_turns: 1
codex:
  command: codex app-server
claude:
  command: claude
review:
  backend: fake
  gemini_command: gemini
observability:
  logs_root: /tmp/jade-symphony-merge-fixture-logs
---

Fixture workflow for `merge-once --dry-run`.

This workflow never performs a live merge. It exists to rehearse merge-lane
decision logic from fixture-linked pull request metadata.
