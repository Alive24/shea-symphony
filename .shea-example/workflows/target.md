---
tracker:
  kind: memory
workspace:
  root: worktrees
artifacts:
  root: artifacts
observability:
  logs_root: logs
runtime_profile:
  path: ../runtime-profile.json
  required: false
  timeout_ms: 10000
---

# Target Repository Shea Workflow

Use this workflow as the target repository's local Shea runtime entrypoint.
Prompts live under `../prompts` relative to this initialized workflow location.
