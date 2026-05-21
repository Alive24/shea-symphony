---
tracker:
  kind: memory
  fixture_path: fixtures/dry-run-issues.json
workspace:
  root: /tmp/jade-symphony-profile-workspaces
main_lane:
  backend: dry-run
profiles:
  default: codex-alpha
  cockpit_tools:
    codex_instances_path: fixtures/cockpit-tools-codex-instances.json
  entries:
    - id: fallback-local
      instance_name: Fallback Local
      backend: dry-run
      workspace_namespace: fallback-local
      env:
        JADE_SYMPHONY_FIXTURE_PROFILE: fallback-local
observability:
  logs_root: /tmp/jade-symphony-profile-logs
---

Use the issue contract and keep this fixture workflow dry-run only.
