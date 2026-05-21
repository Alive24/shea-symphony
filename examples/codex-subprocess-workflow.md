---
tracker:
  kind: github_project_v2
  owner: Alive24
  repo: jade-symphony
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
  root: /tmp/jade-symphony-codex-subprocess-workspaces
main_lane:
  backend: codex
  max_concurrent_agents: 1
  max_turns: 1
  max_retry_backoff_ms: 300000
codex:
  command: "cat > codex-subprocess-output.md"
  turn_timeout_ms: 1000
  approval_policy:
    mode: dry-run-fixture
  thread_sandbox: workspace-write
observability:
  logs_root: log
---

You are running a safe fixture task for {{ issue.identifier }}.

Title: {{ issue.title }}
