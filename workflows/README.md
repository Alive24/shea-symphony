# Jade Symphony Workflows

`workflows/jade-symphony.md` is the canonical normal operator workflow index for
Jade Symphony self-dogfood. It owns shared tracker, artifact, workspace, review,
verification, and observability config, then points each lane at its own prompt
contract under `workflows/prompts/`.

Use it for live Project #9 operations:

```bash
cargo run -- autopilot plan workflows/jade-symphony.md
cargo run -- autopilot loop workflows/jade-symphony.md --max-iterations 1 --write
cargo run -- forge validate --workflow workflows/jade-symphony.md --status Todo --title "<title>" --body-file /private/tmp/issue.md
cargo run -- forge validate --workflow workflows/jade-symphony.md --issue '#123' --status Todo --title "<candidate title>" --body-file /private/tmp/candidate.md
cargo run -- forge create --workflow workflows/jade-symphony.md --status Todo --title "<title>" --body-file /private/tmp/issue.md --assignee Alive24 --write
cargo run -- main loop workflows/jade-symphony.md --max-iterations 1 --write
cargo run -- review loop workflows/jade-symphony.md --max-iterations 1 --write
cargo run -- merge once workflows/jade-symphony.md --write
cargo run -- main claim workflows/jade-symphony.md '#123' --worker "Codex Manual Main" --write
cargo run -- session start workflows/jade-symphony.md '#123' --lane main --run <RUN_ID> --write
cargo run -- session list workflows/jade-symphony.md
```

Normal all-lane dogfood starts with read-only `autopilot plan`, then uses
bounded foreground `autopilot loop --write`. `autopilot loop` is not a daemon,
background service, or app-server; it composes Main, Review, and Merge lane
ticks in order and returns control to the operator after the explicit iteration
budget. Use `main loop`, `review loop`, or `merge loop` directly for focused
debugging, break-glass recovery, or deliberately lane-specific dogfood.

Main Agent execution uses the configured Main backend. A bounded write tick
creates or resumes the issue runtime, records durable runtime artifacts, and
leaves the issue active until real implementation evidence is available for the
normal handoff path. Status commands classify registered sessions from bounded
runtime evidence while keeping raw backend scrollback/protocol details out of
routine output.
Gemini-backed `review loop` uses the headless CLI path by default: it writes the
Review prompt through stdin, requests JSON output, applies configured model and
interim allowed-tools settings, and records stdout/stderr/job evidence for the
review handoff.
Main-lane crash recovery is enabled by default for bounded `main loop --write`
ticks. It restarts recoverable interrupted `In Progress` tmux runtime slots as
new attempts while preserving issue state, dirty worktrees, and existing claim
evidence. Use `--no-recover` only for debugging or a deliberately conservative
operator pass. Manual tmux recovery remains a two-step break-glass path: use
`main claim`, `review claim`, or `merge claim` to write the matching Project
claim field, print the structured `run=`, and record minimum non-tmux registry
evidence for the manual Codex App claim. Worker labels may be human-readable
display labels with spaces; claim commands quote and validate those values
before Project writes. Then use `session start --lane ... --run ...` to render
the lane prompt and start the tmux runtime when a supervised terminal is needed.
Session commands validate the existing claim and write attach/log evidence
without approving reviews, merging PRs, or closing issues.

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
