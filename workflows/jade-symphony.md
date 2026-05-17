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
    allow_unassigned: false
    assignees: []
  workpad:
    source: issue_comment
    marker: "<!-- jade-symphony-workpad -->"
prompts:
  main_agent: prompts/main-agent.md
  review_agent: prompts/review-agent.md
  merge_agent: prompts/merge-agent.md
polling:
  interval_ms: 5000
artifacts:
  root: $JADE_SYMPHONY_ARTIFACT_ROOT
  namespace: Alive24/jade-symphony
workspace:
  root: $JADE_SYMPHONY_ARTIFACT_ROOT/Alive24/jade-symphony/default/worktrees
agent:
  backend: tmux
  max_concurrent_agents: 1
  max_turns: 3
  max_retry_backoff_ms: 300000
tmux:
  command: tmux
  agent_command: codex
  review_agent_command: $JADE_GEMINI_COMMAND
  session_prefix: jade
codex:
  command: codex app-server
claude:
  command: claude
review:
  backend: gemini-cli
  gemini_command: $JADE_GEMINI_COMMAND
  timeout_ms: 600000
  max_concurrent_workers: 2
verification:
  timeout_ms: 600000
  commands: []
observability:
  logs_root: $JADE_SYMPHONY_ARTIFACT_ROOT/Alive24/jade-symphony/default/logs
---

# Jade Symphony Workflow Index

This is the canonical normal operator workflow for Jade Symphony Project #9.
The front matter above owns shared tracker, workspace, review, verification,
artifact, and observability configuration. Agent behavior is intentionally split
by lane so each command initializes with the contract that matches its authority
boundary.

Lane prompt contracts:

- Main Agent: `workflows/prompts/main-agent.md`
- Review Agent: `workflows/prompts/review-agent.md`
- Merge Agent: `workflows/prompts/merge-agent.md`

Older fixture workflows may still keep an inline prompt body. This canonical
workflow uses explicit lane prompts so Main, Review, and Merge agents do not
share one implicit main-agent prompt.
