# Dream Run: SSH Worker Workspaces

Date: 2026-05-19
Run: `2026-05-19-03-ssh-worker-workspaces`
Mode: write-mode Dream continuation
Operator context: continue OpenAI Symphony reference comparison after worker-supervision parity

## Source Inventory

- `git status --short --branch`
- `cargo run -- project state workflows/jade-symphony.md`
- `cargo run -- project issue workflows/jade-symphony.md '#325' --json`
- `gh issue list --repo Alive24/jade-symphony --state open --search "SSH OR remote OR worker host OR workspace lifecycle OR workspace cleanup OR worktree cleanup OR hooks" --json number,title,url,state --limit 100`
- `gh issue view 253 --repo Alive24/jade-symphony --json number,title,state,body,url`
- `gh issue view 271 --repo Alive24/jade-symphony --json number,title,state,body,url`
- `docs/bootstrap/references/openai-symphony/elixir/README.md`
- `docs/bootstrap/references/openai-symphony/elixir/WORKFLOW.md`
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/ssh.ex`
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/codex/app_server.ex`
- `docs/bootstrap/references/openai-symphony/elixir/lib/symphony_elixir/config/schema.ex`
- `docs/implementation_notes.md`
- `docs/dogfood-readiness.md`

## Round Summary

This round compared OpenAI Symphony's remote worker path against Jade's current
local workspace model. The reference has a concrete SSH transport helper, remote
app-server launch support, remote workspace validation, worker host config, and
live E2E coverage that can run against disposable localhost SSH workers or real
`SYMPHONY_LIVE_SSH_WORKER_HOSTS`.

Jade has strong local workspace pieces: path safety, hooks, cleanup planning,
`workspace show/adopt/ensure`, canonical checkout guards, and parsed worker SSH
host config. The missing boundary is live remote execution: host scheduling,
remote workspace preparation/evidence, remote sandbox expectations, and how
remote workers integrate with lane pools and later supervision.

## Created Backlog

- #325 `Backlog: shape remote SSH worker workspaces`

## Watchlist / Not Created

- Local workspace discovery/adoption: not created because #253 is closed and
  covers `workspace list/show/adopt`.
- Local Review/Merge workspace preparation: not created because #271 is closed
  and covers `workspace ensure`, canonical checkout guards, and local
  Review/Merge inspection.
- Persistent worker supervision: not created because #324 now covers the
  broader supervision boundary.
- Codex app-server transport and continuation: not created because #321 covers
  that parity seed.

## Doctor / Project Warnings

- `project state` after #325 reports `Agent Review:1, Backlog:15, Done:77,
  Todo:4`.
- The canonical checkout remains clean but local main is ahead of `origin/main`
  because Dream Log commits are local.
- The prior Doctor warning set remains relevant: #243 terminal Review Agent
  claim missing registry evidence and local Codex/Gemini skill install drift.

## Gemini Review Status

Gemini review passed. See `gemini-review.md`.

## Slept Enough

Slept enough: no.

Reason: the SSH/workspace round created the one clear remote-worker seed, but a
final parity-table audit should still check remaining partial rows such as
prompt rendering, live tracker adapters/smoke tests, runtime state resume
wiring, and operator status snapshots before declaring the source window quiet.

## Safety Notes

Dream-created issue #325 stayed in `Backlog`. It was not promoted, claimed, or
treated as executable lane work. Project state and issue creation were verified
through the Jade Symphony CLI.
