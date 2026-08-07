---
tracker:
  kind: github_project_v2
  owner: Alive24
  repo: shea-symphony
  project_owner: Alive24
  project_owner_type: user
  project_number: 9
  status_field: Status
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
    additional_assignees: []
  workpad:
    source: issue_comment
    marker: "<!-- shea-symphony-workpad -->"
git:
  base_branch: main
temporal:
  address: localhost:7233
  namespace: default
  task_queues:
    core: symphony-core
    agent: symphony-agent
    local: symphony-local
  worker:
    core_concurrency: 3
    agent_concurrency: 3
    local_concurrency: 8
prompts:
  main_agent: ../prompts/main-agent.md
  review_agent: ../prompts/review-agent.md
  merge_agent: ../prompts/merge-agent.md
workpad_templates:
  agent_review_run: ../template/workpad/agent-review.md
  doctor_triage: ../template/workpad/doctor-triage.md
  human_review_repair: ../template/workpad/doctor-triage.md
  merge_run: ../template/workpad/merge-run.md
  merge_repair: ../template/workpad/merge-run.md
  forge_rework_run: ../template/workpad/rework-run.md
  forge_rework_blocked: ../template/workpad/rework-run.md
polling:
  interval_ms: 50000
artifacts:
  root: ../artifacts
  namespace: Alive24/shea-symphony
workspace:
  root: ../worktrees
main_lane:
  backend: codex
  max_concurrent_agents: 3
  max_turns: 3
  max_retry_backoff_ms: 300000
codex:
  command: codex app-server -c 'service_tier="fast"'
  reasoning_effort: high
  approval_policy: never
  stall_timeout_ms: 300000
  session_stale_after_ms: 1800000
claude:
  command: claude
review_lane:
  backend: agy-cli
  agy_command: /Users/chuntengxiao/.local/bin/agy
  agy_model: "Gemini 3.1 Pro (High)"
  codex_approval_policy: never
  codex_thread_sandbox: read-only
  timeout_ms: 1200000
  max_concurrent_workers: 2
merge_lane:
  agent_backend: codex
  max_concurrent_workers: 3
verification:
  timeout_ms: 600000
  commands: []
observability:
  logs_root: ../logs
---

# Shea Symphony Workflow Index

This is the canonical normal operator workflow for Shea Symphony Project #9.
The front matter above owns shared tracker, workspace, review, verification,
artifact, and observability configuration. Agent behavior is intentionally split
by lane so each command initializes with the contract that matches its authority
boundary.

Lane prompt contracts:

- Main Agent: `../prompts/main-agent.md`
- Review Agent: `../prompts/review-agent.md`
- Merge Agent: `../prompts/merge-agent.md`

Older fixture workflows may still keep an inline prompt body. This canonical
workflow uses explicit lane prompts so Main, Review, and Merge agents do not
share one implicit main-agent prompt.
