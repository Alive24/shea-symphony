# Dream Topic: SSH Worker Workspaces

## Theme

OpenAI Symphony has a concrete remote worker path. Jade has local workspace
safety and parsed remote config, but not live SSH execution or remote workspace
evidence.

## Evidence Anchors

- `docs/bootstrap/references/openai-symphony/elixir/README.md`: live E2E runs
  one local-worker scenario and one SSH-worker scenario, using disposable
  localhost workers unless `SYMPHONY_LIVE_SSH_WORKER_HOSTS` is set.
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/ssh.ex`:
  provides SSH command and port launch helpers with host/port parsing.
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/codex/app_server.ex`:
  can launch app-server remotely, validates remote workspace strings, and uses
  remote sandbox policy.
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/config/schema.ex`:
  includes `worker.ssh_hosts` and `worker.max_concurrent_agents_per_host`.
- `docs/implementation_notes.md`: marks workspace lifecycle hooks with
  timeout/remote SSH parity as Partial.
- `docs/dogfood-readiness.md`: says Jade parses but does not use SSH worker host
  config and has no live SSH execution.

## Candidate Triage

### Remote SSH Worker Workspaces

- Backlog seed: #325
- Dream confidence: Medium
- Why kept: #253 and #271 solved local workspace discovery and ensure flows but
  explicitly left remote/SSH workspaces out of scope. The reference has enough
  concrete transport and E2E behavior to justify a Backlog seed.
- Promotion path: Issue Forge should choose the first safe remote boundary:
  command transport, host scheduling, remote workspace prep, remote sandbox, or
  disposable localhost SSH E2E.

## Existing Coverage Checked

- #253: closed local workspace discovery/adoption; remote workspace discovery
  out of scope.
- #271: closed local `workspace ensure`; remote/SSH workspaces out of scope.
- #321: app-server continuation parity, not remote host scheduling.
- #324: persistent worker supervision boundary, not SSH transport/workspace
  prep.
- Open issue search did not find a dedicated remote SSH worker Backlog/Todo.

## Coverage Decision

One seed is enough. Remote SSH transport, host scheduling, remote sandbox, and
remote workspace evidence should stay together until Issue Forge selects the
first execution slice.

## Lane-Authority Note

This topic log is advisory only. Main, Review, Merge, and Doctor should not
treat it as an invariant unless the seed is later promoted into an issue
contract or a repo-owned doc/skill/CLI check.
