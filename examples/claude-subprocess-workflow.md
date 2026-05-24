---
tracker:
  kind: github_project_v2
  owner: Alive24
  repo: shea-symphony
  project_owner: Alive24
  project_number: 9
  fixture_path: fixtures/dry-run-issues.json
  status_field: Status
  state_map:
    todo: Todo
    rework: Rework
    done: Done
  active_states:
    - Todo
    - Rework
  terminal_states:
    - Done
workspace:
  root: /tmp/shea-symphony-claude-subprocess-workspaces
main_lane:
  backend: claude-code
  max_concurrent_agents: 1
  max_turns: 1
  max_retry_backoff_ms: 300000
claude:
  command: "cat > claude-subprocess-output.md"
  turn_timeout_ms: 1000
observability:
  logs_root: log
---

You are running a safe Claude Code fixture task for {{ issue.identifier }}.

Title: {{ issue.title }}
