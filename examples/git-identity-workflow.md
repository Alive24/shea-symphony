---
tracker:
  kind: memory
  fixture_path: fixtures/git-identity-issues.json
  state_map:
    todo: Todo
    in_progress: In Progress
    agent_review: Agent Review
    need_human_input: Need Human Input
  active_states:
    - Todo
    - Rework
  assignee_filter:
    source: issue_assignees
    allow_unassigned: true
workspace:
  root: /tmp/shea-symphony-git-identity-workspaces
hooks:
  after_create: git init
main_lane:
  backend: dry-run
  max_concurrent_agents: 1
  max_turns: 3
  max_retry_backoff_ms: 300000
identity:
  actor_role: implementation_agent
  actor_label: Shea Symphony Dry Run Agent
  git:
    name: Shea Symphony Agent
    email: shea-symphony-agent@example.invalid
    extra:
      shea.actorRole: implementation_agent
observability:
  logs_root: /tmp/shea-symphony-git-identity-logs
---

You are working on Shea Symphony issue {{ issue.identifier }}.

Title: {{ issue.title }}
State: {{ issue.state }}

Use the configured actor identity only for local workspace git metadata and
runtime evidence. Do not write global git config or log secrets.
