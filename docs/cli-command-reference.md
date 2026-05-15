# CLI Command Reference

This reference describes the current `jade-symphony` command surface on `main`.
It is organized by operator task and safety boundary rather than by parser
order.

All live tracker mutations require explicit `--write`. Fixture-backed workflows
remain the preferred rehearsal path for local development. `doctor` can omit the
workflow path when `JADE_SYMPHONY_WORKFLOW` is set, or when
`workflows/jade-symphony.md` exists in the current repo checkout.

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
| `doctor` | Audit Project/workflow/runtime invariants. | `cargo run -- doctor` |
| `audit-project` | Compatibility alias for `doctor`. | `cargo run -- audit-project workflows/jade-symphony.md` |
| `profiles` | List configured/discovered execution profiles. | `cargo run -- profiles examples/cockpit-profiles-workflow.md` |

Doctor repair helpers:

```bash
cargo run -- doctor --interactive
cargo run -- doctor --auto-fix --dry-run
cargo run -- doctor --auto-fix --write
cargo run -- doctor repair 194
cargo run -- doctor repair 194 --move-need-human-input --write
```

`doctor repair ISSUE` is non-destructive by default. It prints safe,
uncertain, and dangerous actions for the issue using tracker state, Project
fields, runtime-state evidence, branch/PR hints, and doctor findings. The
`--move-need-human-input --write` path writes workpad evidence before changing
tracker state.

## Main Implementation Runtime

| Command | Purpose | Boundary |
| --- | --- | --- |
| `run-once` | Execute one selected issue through the configured backend. | Fixture-safe by default when the workflow has `tracker.fixture_path`. |
| `run-loop` | Poll/select/claim/run/handoff in bounded or idle-loop modes. | Live write mode requires `--write`; `--pool N` filters by `Main Agent`; main-agent completion stops at `Agent Review`. |
| `dogfood-smoke` | Supervised preflight for one controlled dogfood issue. | Dry-run inspection by default; live readiness does not bypass review or merge gates. |

Examples:

```bash
cargo run -- run-once examples/dry-run-workflow.md
cargo run -- run-loop examples/dry-run-workflow.md --max-iterations 1 --dry-run
cargo run -- run-loop examples/dry-run-workflow.md --max-iterations 1 --dry-run --display tui
cargo run -- run-loop workflows/jade-symphony.md --max-iterations 1 --pool 2 --dry-run
cargo run -- dogfood-smoke workflows/jade-symphony.md --dry-run
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
before tracker mutation. `run-loop`, `review-loop`, and `merge-once` print
compact `Latest:` status bars in addition to their detailed line logs.

## Tracker Writes

These commands can mutate live tracker state and require `--write`.

| Command | Purpose | Boundary |
| --- | --- | --- |
| `set-state` | Move one issue to a normalized workflow state. | Refuses `Human Review` from the main implementation role. |
| `workpad` | Upsert the canonical issue workpad comment. | Use for durable evidence before state transitions. |
| `create-follow-up` | Create a follow-up issue from a body file. | Lower-level creation path; prefer `forge-create` for quality-gated issues. |
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
| `clean audit` | Classify configured artifact/workspace residue by persistence action. | Read-only; categories include `promote_to_repo`, `attach_to_tracker`, `safe_to_keep`, `cleanup_candidate`, and `needs_human_decision`. |

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
| `forge-discover` | Discover local candidate issue intent from free text. | Read-only. |
| `forge-discuss` | Ask focused clarification for a draft issue. | Read-only. |
| `forge-draft` | Build a quality-template issue draft from title/goal. | Read-only. |
| `forge-validate` | Validate an issue body against the quality gate. | Read-only. |
| `forge-repair` | Repair thin Markdown into an executable issue shape. | Read-only. |
| `forge-interactive` | Conversation-first issue shaping from natural-language intent. | Starts with only `--workflow`; creation requires `--write --confirm-create --assignee`. |
| `forge-reflect` | Reflect over local context and print candidate issues. | Read-only. |
| `forge-create` | Create a quality-gated tracker issue. | Requires explicit `--write`; live GitHub creation requires `--assignee`; can add to Project with `--add-to-project`. |

Examples:

```bash
cargo run -- forge-validate --title "Thin Forge issue" --file examples/fixtures/thin-issue.md
cargo run -- forge-interactive --workflow workflows/jade-symphony.md
cargo run -- forge-interactive --workflow workflows/jade-symphony.md --intent "make run-loop explain retry backoff better"
cargo run -- forge-reflect --context-file docs/dogfood-readiness.md --limit 1
cargo run -- forge-create --workflow workflows/jade-symphony.md --title "Follow-up title" --file /tmp/issue.md --assignee Alive24 --add-to-project --write
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
| `review-freshness` | Record/inspect review freshness evidence. | Used around merging/rework conflict repair. |

Example:

```bash
cargo run -- review-loop examples/review-fixture-workflow.md --max-iterations 1 --dry-run
cargo run -- review-loop workflows/jade-symphony.md --max-iterations 1 --write
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
