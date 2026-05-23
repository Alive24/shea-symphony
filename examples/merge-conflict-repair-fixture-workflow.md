---
tracker:
  kind: memory
  owner: Alive24
  repo: jade-symphony
  project_owner: Alive24
  project_number: 9
  status_field: Status
  fixture_path: fixtures/merge-conflict-repair-issues.json
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
  root: /tmp/jade-symphony-merge-conflict-fixture-workspaces
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
  logs_root: /tmp/jade-symphony-merge-conflict-fixture-logs
---

Fixture workflow for controlled `DIRTY` PR merge-lane repair rehearsal.

This workflow never changes a live branch. In write mode, the merge lane records
synthetic safe-conflict-repair evidence and keeps the issue in `Merging` so the
next loop can re-evaluate mergeability, matching the live retry contract.
