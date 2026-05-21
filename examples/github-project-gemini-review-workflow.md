---
tracker:
  kind: github_project_v2
  owner: Alive24
  repo: jade-symphony
  project_owner: Alive24
  project_number: 9
  status_field: Status
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
    - Agent Review
  terminal_states:
    - Done
    - Closed
    - Cancelled
    - Canceled
    - Duplicate
  assignee_filter:
    source: issue_assignees
    allow_unassigned: false
    assignees: []
  workpad:
    source: issue_comment
    marker: "<!-- jade-symphony-workpad -->"
polling:
  interval_ms: 5000
artifacts:
  root: $JADE_SYMPHONY_ARTIFACT_ROOT
workspace:
  root: $JADE_SYMPHONY_ARTIFACT_ROOT/Alive24/jade-symphony/review/worktrees
main_lane:
  backend: dry-run
  max_concurrent_agents: 1
  max_turns: 1
  max_retry_backoff_ms: 300000
review_lane:
  backend: gemini-cli
  gemini_command: $JADE_GEMINI_COMMAND
  timeout_ms: 600000
  max_concurrent_workers: 2
identity:
  actor_role: review_agent
  actor_label: Gemini Review Agent
observability:
  logs_root: $JADE_SYMPHONY_ARTIFACT_ROOT/Alive24/jade-symphony/review/logs
---

You are the independent Review Agent for Jade Symphony issue
{{ issue.identifier }}.

Review the linked implementation evidence and issue contract without performing
main-agent implementation work. Record confirmed findings precisely, preserve
the Human Review authority boundary, and leave inconclusive or unavailable
review evidence in Agent Review rather than advancing the issue.
