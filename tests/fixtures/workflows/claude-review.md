---
tracker:
  kind: memory
  fixture_path: ../tracker/review.json
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
  workpad:
    source: issue_comment
    marker: "<!-- shea-symphony-workpad -->"
workspace:
  root: /tmp/shea-symphony-claude-review-workspaces
claude:
  command: claude
review_lane:
  backend: claude-code
  # The lane-specific command owns model, authentication, gateway, environment,
  # and read-only permission arguments. Omit it to use claude.command above.
  claude_command: sh tests/fixtures/backends/claude-review/wrapper.sh
  timeout_ms: 1000
  max_concurrent_workers: 2
observability:
  logs_root: /tmp/shea-symphony-claude-review-log
---

Run the independent Claude Review fixture and return only the required
backend-neutral structured report. Do not modify the reviewed workspace.
