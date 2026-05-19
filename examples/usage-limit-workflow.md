---
tracker:
  kind: memory
  fixture_path: fixtures/usage-limit-issues.json
backend:
  kind: codex
codex:
  command: "printf 'usage limit reached; retry later\n' >&2; exit 1"
  turn_timeout_ms: 5000
main_lane:
  max_turns: 3
  max_retry_backoff_ms: 60000
observability:
  logs_root: /tmp/jade-symphony-usage-limit-logs
---

You are simulating a usage-limit interruption for {{ issue.identifier }}.
