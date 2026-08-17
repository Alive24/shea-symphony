---
tracker:
  kind: memory
  fixture_path: ../tracker/dry-run.json
workspace:
  root: /tmp/shea-symphony-profile-workspaces
main_lane:
  backend: dry-run
profiles:
  default: codex-alpha
  cockpit_tools:
    codex_instances_path: ../profiles/cockpit-tools-codex-instances.json
  entries:
    - id: fallback-local
      instance_name: Fallback Local
      backend: dry-run
      workspace_namespace: fallback-local
      env:
        SHEA_SYMPHONY_FIXTURE_PROFILE: fallback-local
observability:
  logs_root: /tmp/shea-symphony-profile-logs
---

Use the issue contract and keep this fixture workflow dry-run only.
