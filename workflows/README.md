# Shea Symphony Workflows

`workflows/shea-symphony.md` is the canonical normal operator workflow index for
Shea Symphony self-dogfood. It owns shared tracker, artifact, workspace, review,
verification, and observability config, then points each lane at its own prompt
contract under `workflows/prompts/`.

Use it for live Project #9 operations:

```bash
cargo run -- autopilot plan workflows/shea-symphony.md
cargo run -- autopilot loop workflows/shea-symphony.md --max-iterations 1 --write
cargo run -- forge validate --workflow workflows/shea-symphony.md --status Todo --title "<title>" --body-file /private/tmp/issue.md
cargo run -- forge validate --workflow workflows/shea-symphony.md --issue '#123' --status Todo --title "<candidate title>" --body-file /private/tmp/candidate.md
cargo run -- forge create --workflow workflows/shea-symphony.md --status Todo --title "<title>" --body-file /private/tmp/issue.md --assignee Alive24 --write
cargo run -- main loop workflows/shea-symphony.md --max-iterations 1 --write
cargo run -- review loop workflows/shea-symphony.md --max-iterations 1 --write
cargo run -- merge once workflows/shea-symphony.md --write
cargo run -- main claim workflows/shea-symphony.md '#123' --worker "Codex Manual Main" --write
cargo run -- session start workflows/shea-symphony.md '#123' --lane main --run <RUN_ID> --write
cargo run -- session list workflows/shea-symphony.md
```

Normal all-lane dogfood starts with read-only `autopilot plan`, then uses
bounded foreground `autopilot loop --write`. `autopilot loop` is not a daemon,
background service, or app-server; it composes Main, Review, and Merge lane
ticks in order and returns control to the operator after the explicit iteration
budget. Use `main loop`, `review loop`, or `merge loop` directly for focused
debugging, break-glass recovery, or deliberately lane-specific dogfood.

Write-mode lane/control commands are safe to run from the canonical checkout on
`main` even when local `main` is only behind `origin/main`: before tracker
mutation, Shea Symphony fetches the configured upstream and performs a
canonical-only `git merge --ff-only`. Dry-runs report `would_ff_only` without
changing the checkout. Dirty, detached, non-`main`, missing-upstream, and
non-fast-forward cases still fail closed; issue worktrees and PR branches are
not refreshed by this path.

Main Agent execution defaults to the Codex app-server backend through
`main_lane.backend: codex`, `codex.command: codex app-server -c 'service_tier="fast"'`, and
`codex.approval_policy: never`. A bounded write tick creates or resumes the
issue worktree, runs one app-server turn, records prompt/protocol/stderr/
normalized-event artifacts, persists a backend session registry record, and
reconciles the normal Main handoff only after PR readiness and linked-PR
readback are proven. `main_lane.backend: tmux` remains an explicit
fallback/debug setting, not the unattended default.
Gemini-backed `review loop` uses the headless CLI path by default: it writes the
Review prompt through stdin, requests JSON output, applies configured model and
interim allowed-tools settings, and records stdout/stderr/job evidence for the
review handoff.
Main-lane crash recovery is enabled by default for bounded `main loop --write`
ticks. It restarts recoverable interrupted `In Progress` runtime slots as new
attempts while preserving issue state, dirty worktrees, and existing claim
evidence. Codex app-server session staleness defaults to 30 minutes and can be
configured with `codex.session_stale_after_ms`; stale registered app-server
processes are terminated before recovery resumes the saved thread with
`Continue`. Codex app-server turn inactivity defaults to 5 minutes and can be
configured with `codex.stall_timeout_ms`; a turn that starts but emits no further
protocol events is terminated and treated as retryable lane backend failure.
Use `--no-recover` only for debugging or a deliberately conservative
operator pass. Manual session recovery remains a two-step break-glass path: use
`main claim`, `review claim`, or `merge claim` to write the matching Project
claim field, print the structured `run=`, and record minimum non-tmux registry
evidence for the manual Codex App claim. Worker labels may be human-readable
display labels with spaces; claim commands quote and validate those values
before Project writes. Then use `session start --lane ... --run ...` to render
the lane prompt and start the configured runtime when a supervised session is
needed. Main and Merge-agent sessions default to Codex app-server through the
lane backend config and `codex.command`; Review session start remains the tmux
supervised fallback while automatic Review uses Gemini headless.
Clean `merge once` / `merge loop` remains direct CLI merge behavior and does not
launch a merge-agent runtime. Dirty PRs still try the mechanical direct-CLI
repair first; only content conflicts in a trusted clean PR worktree launch the
configured merge-agent backend. Interrupted conflict-repair attempts are
aborted back to a clean PR-branch baseline on the next tick before retrying, and
retryable backend or verification failures stay in `Merging` instead of asking
for human input when no semantic decision is required. Session commands validate
the existing claim and write runtime evidence without approving reviews, merging
PRs, or closing issues.

Merge-lane crash recovery is enabled by default for bounded
`merge loop --write` ticks. It adopts interrupted structured merge-loop/goal
claims first, leaves manual claims alone, keeps merge-lane transport repair
inside `Merging` or `Need Human Input`, and never routes merge repair through
`Rework`.

Lane prompt files:

- `workflows/prompts/main-agent.md`: implementation agent contract; stops at
  `Agent Review`.
- `workflows/prompts/review-agent.md`: independent review contract; may route
  passing work to `Human Review` only with review evidence.
- `workflows/prompts/merge-agent.md`: guarded landing contract for `Merging`
  issues only.

`examples/` is for fixture workflows, demos, and compatibility references.
Older examples may keep inline prompt bodies for compatibility. Do not add a
second normal dogfood workflow for a specific lane; lane selection belongs in
the command controller and this workflow config.

`autopilot loop` reads `polling.interval_ms`,
`main_lane.max_concurrent_agents`, `review_lane.max_concurrent_workers`, and
`merge_lane.max_concurrent_workers` from this workflow unless explicit CLI
overrides are provided. It remains bounded and foreground-only; mutating lane
ticks require `--write`.
