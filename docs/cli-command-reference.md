# CLI Command Reference

This reference describes the current `jade-symphony` command surface on `main`.
It is organized by operator task and safety boundary rather than by parser
order.

All live tracker mutations require explicit `--write`. Fixture-backed workflows
remain the preferred rehearsal path for local development. `doctor` can omit the
workflow path when `JADE_SYMPHONY_WORKFLOW` is set, or when
`workflows/jade-symphony.md` exists in the current repo checkout.

For normal dogfood, Jade Symphony CLI is the authority for GitHub Project v2
workflow reads and mutations. Direct `gh issue view` / `gh pr view` is still
acceptable for raw issue or PR content, but Project status, Project fields,
relationships, claim locks, workpads, and state transitions should go through
the commands in this reference. Manual Project UI or raw Project GraphQL changes
are break-glass recovery actions, not the standard path.

The canonical `workflows/jade-symphony.md` file is a workflow index/config. It
references lane-specific prompts in `workflows/prompts/` so Main, Review, and
Merge commands initialize with their own authority contracts. Older fixture
workflows may still use an inline prompt body.

## Read-Only Planning And Inspection

| Command | Purpose | Example |
| --- | --- | --- |
| `plan` | Default dispatch/status plan for a workflow. | `cargo run -- plan examples/dry-run-workflow.md` |
| `plan-dispatch` | Alias-style dispatch planning command. | `cargo run -- plan-dispatch examples/dry-run-workflow.md` |
| `dry-run` | Compatibility alias for planning output. | `cargo run -- dry-run examples/dry-run-workflow.md` |
| `status` | Operator status alias for planning output. | `cargo run -- status examples/dry-run-workflow.md` |
| `validate` | Validate workflow loading/configuration. | `cargo run -- validate examples/dry-run-workflow.md` |
| `validate-workflow` | Compatibility alias for `validate`. | `cargo run -- validate-workflow examples/dry-run-workflow.md` |
| `inspect` | Read tracker issues and print gate/status information. | `cargo run -- inspect workflows/jade-symphony.md` |
| `project-state` | Diagnose whether the canonical Project read path is trustworthy. | `cargo run -- project-state workflows/jade-symphony.md` |
| `project-issue` | Read one issue's normalized Project state, fields, blockers, and linked PRs through Jade Symphony. | `cargo run -- project-issue workflows/jade-symphony.md '#235' --json` |
| `doctor` | Audit Project/workflow/runtime invariants. | `cargo run -- doctor` |
| `audit-project` | Compatibility alias for `doctor`. | `cargo run -- audit-project workflows/jade-symphony.md` |
| `profiles` | List configured/discovered execution profiles. | `cargo run -- profiles examples/cockpit-profiles-workflow.md` |
| `debug` | Read-only human report combining Project, doctor, smoke readiness, runtime/session, cleanup, and lane next-action signals. | `cargo run -- debug workflows/jade-symphony.md` |

Doctor repair helpers:

```bash
cargo run -- doctor --interactive
cargo run -- doctor --auto-fix --dry-run
cargo run -- doctor --auto-fix --write
cargo run -- doctor repair 194
cargo run -- doctor repair 194 --move-need-human-input --write
cargo run -- doctor repair 194 --mark-pr-ready --confirm-handoff-ready --write
```

`doctor repair ISSUE` is non-destructive by default. It prints safe,
uncertain, and dangerous actions for the issue using tracker state, Project
fields, runtime-state evidence, branch/PR hints, and doctor findings. The
`--move-need-human-input --write` path writes workpad evidence before changing
tracker state.
`--mark-pr-ready --confirm-handoff-ready --write` is an explicit operator repair
for `Agent Review` issues whose linked PR is still draft. It writes repair
evidence and runs `gh pr ready`; `doctor --auto-fix` never marks PRs ready.

## Main Implementation Runtime

| Command | Purpose | Boundary |
| --- | --- | --- |
| `run-once` | Execute one selected issue through the configured backend. | Fixture-safe by default when the workflow has `tracker.fixture_path`. |
| `run-loop` | Poll/select/claim/run/handoff in bounded or idle-loop modes. | Live write mode requires `--write` and a real main-agent backend; tmux sessions stay active instead of auto-handing off; Agent Review handoff requires a verified Project-visible, ready, non-draft PR. |

Examples:

```bash
cargo run -- run-once examples/dry-run-workflow.md
cargo run -- run-loop examples/dry-run-workflow.md --max-iterations 1 --dry-run
cargo run -- run-loop examples/dry-run-workflow.md --max-iterations 1 --dry-run --display tui
cargo run -- run-loop workflows/jade-symphony.md --max-iterations 1 --pool 2 --dry-run
cargo run -- clean plan workflows/jade-symphony.md
cargo run -- clean audit workflows/jade-symphony.md
```

Use `--display tui` for an opt-in operator panel on `run-loop`, `project-state`,
and `doctor`. The default output stays line-oriented for logs and scripts.

`run-loop --pool N` is a supervised planning and claim-locking slice. Dry-run
mode previews up to `N` eligible main-lane issues after skipping items whose
`Main Agent` Project field is already owned by another worker. Write mode still
processes one main work item at a time because the runtime state tracks one
active issue, but it uses the same lane claim check and stamps `Main Agent`
before tracker mutation. `run-loop --write`, `review-loop --write`, and
`merge-loop --write` also enforce a clean canonical launch checkout before the
first tracker mutation. Tracked dirty files block the lane; recognized
untracked runtime/log/prompt/evidence/draft artifacts are moved to artifact
quarantine with a warning; unclassified untracked files block for operator
repair. `run-loop`, `review-loop`, and `merge-once` print compact `Latest:`
status bars in addition to their detailed line logs.
New lane claims are written as single-line `v=1` key/value audit pointers, for
example `v=1 lane=main actor=codex source=loop issue=#244 run=... state=active
thread=unknown registry=run/...`. The Project field stores the compact pointer;
the session registry and workpad store the durable paths, logs, and handoff
evidence for the same `run=`.

PR relationship verification is a lane invariant, not just evidence text. A PR
URL found in a workpad, issue comment, or local branch can help operators
identify the intended PR, but the issue must expose that PR through the
Project/issue linked-PR read surface before Main handoff, Review routing, or
Merge landing. If Jade Symphony cannot verify the relationship after a repair
attempt, it routes the issue to `Need Human Input` with the blocker preserved.

The canonical `workflows/jade-symphony.md` file uses the local `tmux` main-agent
backend. A launched tmux session records its session name, log path, workspace,
branch, attach command, prompt artifact, actor, lane, attempt, and running
status in a durable session registry under the configured artifact root. The
registry is terminal-session evidence only; tracker state remains the issue
lifecycle source of truth. The issue stays in the active main lane until later
completion evidence satisfies the existing handoff rules. If an operator
overrides the workflow back to `agent.backend: dry-run`, `run-loop --write`
exits non-zero before loading runtime state, creating worktrees, claiming
Project fields, or writing workpads.

When the tmux agent command is Codex, `run-loop --write` captures the pane before
prompt injection. The default behavior auto-advances the Codex workspace trust
prompt only inside the configured Jade Symphony issue worktree root, then injects the
rendered prompt after a ready viewport is observed. Set
`JADE_SYMPHONY_TMUX_AUTO_TRUST=0` to opt out; a visible trust prompt or missing
readiness then fails closed and preserves attach/log evidence for inspection.

For manual lane recovery, `agent-session start WORKFLOW ISSUE --lane
main|review|merge --write` starts the configured local tmux command with the
lane-specific prompt, writes the lane claim field (`Main Agent`, `Review
Agent`, or `Merging Agent`), and records session evidence in the workpad.
The rendered prompt includes the assigned `run=` and registry pointer so the
spawned agent can preserve that value in its handoff evidence.
`agent-session list WORKFLOW` shows active tmux sessions with attach commands,
and `agent-session attach WORKFLOW SESSION` prints the exact attach command
without joining the terminal unless `--exec` is provided.
`status` and `status-api` include registered tmux session summaries from the
durable session registry. `doctor` flags stale, failed, orphaned, usage-limited,
or runtime/session mismatch cases, while `clean audit` classifies the registry,
rendered prompts, tmux logs, and individual sessions without deleting them.

## Workspace Discovery

Use `workspace` when a lane needs to find the local worktree for an issue before
starting review or merge repair. Discovery combines Project issue/PR hints,
session registry records, canonical workpad evidence, and local
`git worktree list --porcelain` output.

| Command | Purpose | Boundary |
| --- | --- | --- |
| `workspace list` | List issue worktrees and inferred orphan hints. | Read-only. |
| `workspace show` | Show canonical and candidate worktrees for one issue. | Read-only; multiple strong candidates require operator choice. |
| `workspace adopt` | Record an operator-selected local worktree in the issue workpad. | Validates the path is a worktree for this repository and the branch matches the issue. |

Examples:

```bash
cargo run -- workspace list workflows/jade-symphony.md
cargo run -- workspace show workflows/jade-symphony.md '#253'
cargo run -- workspace adopt workflows/jade-symphony.md '#253' /tmp/jade-symphony-issue-253 --write
```

Review lane uses discovered worktrees for read-only inspection by default.
Merge lane should prefer the canonical Main PR worktree/branch for merge-lane
repair instead of creating a replacement workspace. `doctor` warns when multiple
strong candidates exist for one active issue.

## Tracker Writes

These commands can mutate live tracker state and require `--write`.

| Command | Purpose | Boundary |
| --- | --- | --- |
| `set-state` | Move one issue to a normalized workflow state. | Refuses `Human Review` from the main implementation role. |
| `workpad` | Upsert the canonical issue workpad comment. | Use for durable evidence before state transitions. |
| `create-follow-up` | Create a follow-up issue from a body file. | Lower-level creation path; prefer `forge create` for quality-gated issues. |
| `add-to-project` | Add an existing GitHub issue node to the configured Project. | Initializes configured Project status where supported. |

Examples:

```bash
cargo run -- set-state workflows/jade-symphony.md '#123' need_to_clarify --write
cargo run -- workpad workflows/jade-symphony.md '#123' /tmp/workpad.md --write
```

## Clean Lane

`clean` owns local cleanup and persistence-audit concerns. `doctor` remains
focused on tracker/runtime health, stuck workflow states, PR/review/merge
invariants, and repair evidence.

| Command | Purpose | Boundary |
| --- | --- | --- |
| `clean plan` | Grouped read-only alias for `cleanup-plan`. | Reports terminal clean worktrees that are cleanup candidates; never deletes. |
| `cleanup-plan` | Compatibility path for existing scripts. | Same output and read-only boundary as `clean plan`. |
| `clean audit` | Classify configured artifact/workspace residue by persistence action. | Read-only; categories include `promote_to_repo`, `attach_to_tracker`, `safe_to_keep`, `cleanup_candidate`, `needs_human_decision`, and canonical checkout quarantine. |

Examples:

```bash
cargo run -- clean plan workflows/jade-symphony.md
cargo run -- clean audit workflows/jade-symphony.md
```

## Issue Quality Gate

| Command | Purpose | Boundary |
| --- | --- | --- |
| `gate` | Evaluate one issue with the deterministic/optional LLM gate. | Read-only. |
| `gate-apply` | Apply gate failure routing and workpad evidence. | Requires `--write` for tracker mutation. |

Examples:

```bash
cargo run -- gate examples/dry-run-workflow.md '#3'
cargo run -- gate-apply workflows/jade-symphony.md '#123' --write
```

## Issue Forge

| Command | Purpose | Boundary |
| --- | --- | --- |
| `forge validate` | Validate a body file or existing issue for `Backlog` or `Todo`. | Read-only; `Todo` uses the full Issue Quality Gate, `Backlog` uses the lighter seed gate. |
| `forge create` | Create a Project-backed issue in `Backlog` or `Todo`. | Dry-run by default unless `--write` is supplied; live `Todo` creation requires `--assignee`. |
| `forge promote` | Promote one existing Backlog issue in place by editing title/body, moving it to `Todo`, and writing a structured Promotion Note comment. | Dry-run by default unless `--write` is supplied; requires structured note inputs and reports the checkpoint where any failure stopped. |

Examples:

```bash
cargo run -- forge validate --workflow workflows/jade-symphony.md --status Backlog --title "Backlog seed" --body-file /tmp/issue.md
cargo run -- forge validate --workflow workflows/jade-symphony.md --status Todo --title "Executable issue" --body-file /tmp/issue.md
cargo run -- forge create --workflow workflows/jade-symphony.md --status Backlog --title "Backlog: follow-up title" --body-file /tmp/issue.md --dry-run
cargo run -- forge create --workflow workflows/jade-symphony.md --status Todo --title "Follow-up title" --body-file /tmp/issue.md --assignee Alive24 --write
cargo run -- forge promote '#241' --workflow workflows/jade-symphony.md --title "Executable title" --body-file /tmp/issue.md --operator-confirmation "promote it" --decision "Use the CLI-owned promotion note template." --scope-change "Backlog seed is now an executable Todo issue." --dependency-context "Dependencies: none; related context is non-blocking." --dry-run
```

`forge promote` owns the Promotion Note requirement. The command refuses missing
or empty `--operator-confirmation`, `--decision`, `--scope-change`, and
`--dependency-context` values. On write success, the comment uses this short
Markdown shape:

```md
## Promotion Note

- Source Backlog issue: #...
- Promoted Todo title/status: `...` / `Todo`
- Operator confirmation: "..."

## Key Operator Decisions

- ...

## Major Scope Changes From Seed

- ...

## Dependencies and Context

- ...

## Verification Readback

- ...
```

## Review Agent Lane

The main implementation agent must never set `Human Review`. Review commands
represent the independent review lane and must record evidence before status
changes.

| Command | Purpose | Boundary |
| --- | --- | --- |
| `review-fake` | Fixture/fake review transition helper. | Local testing path. |
| `review-once` | Run one configured review backend for one issue. | Only Review Agent may advance passed reviews to `Human Review`. |
| `review-loop` | Bounded review worker selection/reconciliation. | Prevents duplicate review workers where evidence exists. |
| `review-claim` | Claim one `Agent Review` item's `Review Agent` field for manual/operator review. | Requires `--write`; refuses non-`Agent Review` issues. |
| `review-clear-claim` | Clear one issue's `Review Agent` claim through the tracker adapter. | Requires `--write`; use after terminal manual review routing. |
| `review-pass` | Record manual independent review pass evidence and move to `Human Review`. | Requires `--write` and a durable evidence file; writes the doctor-recognized pass marker first. |
| `review-reject` | Record failed/inconclusive manual review evidence and route to `Agent Review`, `Rework`, or `Need Human Input`. | Refuses `Human Review`. |
| `review-freshness` | Record/inspect review freshness evidence. | Used around merging/rework conflict repair. |
| `agent-session start` | Start an attachable local tmux session for a selected lane. | Manual recovery path; it claims only the chosen lane and does not advance workflow state. |
| `agent-session list` | List active Jade Symphony tmux sessions by configured prefix. | Read-only operator summary. |
| `agent-session attach` | Print or execute the tmux attach command for one session. | Defaults to printing the command; `--exec` enters tmux. |

Example:

```bash
cargo run -- review-loop examples/review-fixture-workflow.md --max-iterations 1 --dry-run
cargo run -- review-loop workflows/jade-symphony.md --max-iterations 1 --write
cargo run -- agent-session start workflows/jade-symphony.md '#220' --lane review --write
cargo run -- agent-session list workflows/jade-symphony.md
cargo run -- review-claim workflows/jade-symphony.md '#226' --worker "Manual Gemini Review" --write
cargo run -- review-pass workflows/jade-symphony.md '#226' --evidence-file /tmp/review-evidence.md --write
cargo run -- review-reject workflows/jade-symphony.md '#226' --evidence-file /tmp/review-evidence.md --target-state rework --write
```

## Merge Lane

| Command | Purpose | Boundary |
| --- | --- | --- |
| `merge-once` | Inspect one `Merging` issue, verify a single linked PR, and either merge or route blockers. | Live merge requires explicit `--write`; dirty/failing PRs route to `Rework`, transient `UNKNOWN` mergeability stays in `Merging` for retry, and `Need Human Input` workpads include a concrete question. |
| `land` | Compatibility alias for `merge-once`. | Same boundary as `merge-once`. |

Examples:

```bash
cargo run -- merge-once workflows/jade-symphony.md --dry-run
```

`merge-once` is separate from main implementation and review work. It should
only consume issues already in `Merging`.

## Live Dogfood Boundary

Use `workflows/jade-symphony.md` for Project #9 live reads and explicit
writes. Before running live write commands, confirm:

- the issue contract passes the Issue Quality Gate;
- the command includes `--write`;
- the target status is allowed for the current role;
- the workpad records evidence before state changes;
- the branch/PR belongs to exactly one issue.

Fixture success is useful rehearsal evidence, but it does not prove live GitHub
Project v2 readiness by itself.
