# Jade Symphony Workflows

`workflows/jade-symphony.md` is the canonical normal operator workflow index for
Jade Symphony self-dogfood. It owns shared tracker, artifact, workspace, review,
verification, and observability config, then points each lane at its own prompt
contract under `workflows/prompts/`.

Use it for live Project #9 operations:

```bash
jade-symphony loop workflows/jade-symphony.md --write
jade-symphony forge workflows/jade-symphony.md --interactive
```

The current CLI command names are still the explicit debug/runtime names:

```bash
cargo run -- run-loop workflows/jade-symphony.md --max-iterations 1 --write
cargo run -- forge-interactive --workflow workflows/jade-symphony.md
cargo run -- review-loop workflows/jade-symphony.md --max-iterations 1 --write
cargo run -- merge-once workflows/jade-symphony.md --write
cargo run -- review-session workflows/jade-symphony.md '#123' --write
cargo run -- merge-session workflows/jade-symphony.md '#123' --write
cargo run -- agent-session start workflows/jade-symphony.md '#220' --lane review --write
cargo run -- agent-session list workflows/jade-symphony.md
```

Main Agent execution uses the local `tmux` backend. A bounded write tick creates
an attachable session, prints `tmux attach-session -t ...`, records the session
log path, persists a session registry record, and leaves the issue active until
real implementation evidence is available for the normal handoff path. Status
commands classify registered tmux sessions from bounded pane/log evidence while
keeping full scrollback out of routine output.
The `agent-session` command is the manual tmux recovery path for all lanes:
`main`, `review`, and `merge` each render their own lane prompt, claim the
matching Project field, and leave workflow state unchanged until the lane's
normal evidence path is ready.
`review-session` and `merge-session` are lane-specific shortcuts for the same
session path. They write attach/log evidence without approving reviews, merging
PRs, or closing issues.

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
