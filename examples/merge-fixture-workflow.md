---
tracker:
  kind: memory
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
main_lane:
  backend: dry-run
  max_concurrent_agents: 1
  max_turns: 1
codex:
  command: codex app-server -c 'service_tier="fast"'
claude:
  command: claude
review_lane:
  backend: fake
  gemini_command: gemini
observability:
  logs_root: /tmp/jade-symphony-merge-fixture-logs
---

Fixture workflow for `merge once` and bounded `merge loop` rehearsal.

This workflow never performs a live GitHub merge. In write mode, fixture merge
commands record the same timeline/state/close sequence as a live successful
merge while using synthetic command evidence, so operators can rehearse the
merge lane end-to-end without landing a real pull request.
