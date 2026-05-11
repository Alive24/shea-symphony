---
tracker:
  kind: memory
  fixture_path: fixtures/dry-run-issues.json
  active_states:
    - Todo
    - Rework
    - Agent Review
  terminal_states:
    - Done
polling:
  interval_ms: 5000
workspace:
  root: /tmp/jade-symphony-review-workspaces
agent:
  backend: dry-run
  max_concurrent_agents: 1
  max_turns: 3
  max_retry_backoff_ms: 300000
review:
  backend: fake
  gemini_command: gemini
  timeout_ms: 600000
observability:
  logs_root: examples/log
---

Review Jade Symphony issue {{ issue.identifier }}.
