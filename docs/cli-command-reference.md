# CLI Command Reference

This reference describes the current `jade-symphony` command surface on `main`.
It is organized by operator task and safety boundary rather than by parser
order.

All live tracker mutations require explicit `--write`. Fixture-backed workflows
remain the preferred rehearsal path for local development.

## Read-Only Planning And Inspection

| Command | Purpose | Example |
| --- | --- | --- |
| `plan` | Default dispatch/status plan for a workflow. | `cargo run -- plan examples/dry-run-workflow.md` |
| `plan-dispatch` | Alias-style dispatch planning command. | `cargo run -- plan-dispatch examples/dry-run-workflow.md` |
| `dry-run` | Compatibility alias for planning output. | `cargo run -- dry-run examples/dry-run-workflow.md` |
| `status` | Operator status alias for planning output. | `cargo run -- status examples/dry-run-workflow.md` |
| `validate` | Validate workflow loading/configuration. | `cargo run -- validate examples/dry-run-workflow.md` |
| `validate-workflow` | Compatibility alias for `validate`. | `cargo run -- validate-workflow examples/dry-run-workflow.md` |
| `inspect` | Read tracker issues and print gate/status information. | `cargo run -- inspect examples/github-project-workflow.md` |
| `doctor` | Audit Project/workflow invariants. | `cargo run -- doctor examples/github-project-workflow.md` |
| `audit-project` | Compatibility alias for `doctor`. | `cargo run -- audit-project examples/github-project-workflow.md` |
| `profiles` | List configured/discovered execution profiles. | `cargo run -- profiles examples/cockpit-profiles-workflow.md` |

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
cargo run -- run-loop examples/github-project-workflow.md --max-iterations 1 --pool 2 --dry-run
cargo run -- dogfood-smoke examples/github-project-workflow.md --dry-run
```

`run-loop --pool N` is a supervised planning and claim-locking slice. It
selects up to `N` eligible main-lane issues after skipping items whose
`Main Agent` Project field is already owned by another worker. Write mode stamps
that field before tracker mutation.

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
cargo run -- set-state examples/github-project-workflow.md '#123' need_to_clarify --write
cargo run -- workpad examples/github-project-workflow.md '#123' /tmp/workpad.md --write
```

## Issue Quality Gate

| Command | Purpose | Boundary |
| --- | --- | --- |
| `gate` | Evaluate one issue with the deterministic/optional LLM gate. | Read-only. |
| `gate-apply` | Apply gate failure routing and workpad evidence. | Requires `--write` for tracker mutation. |

Examples:

```bash
cargo run -- gate examples/dry-run-workflow.md '#3'
cargo run -- gate-apply examples/github-project-workflow.md '#123' --write
```

## Issue Forge

| Command | Purpose | Boundary |
| --- | --- | --- |
| `forge-discover` | Discover local candidate issue intent from free text. | Read-only. |
| `forge-discuss` | Ask focused clarification for a draft issue. | Read-only. |
| `forge-draft` | Build a quality-template issue draft from title/goal. | Read-only. |
| `forge-validate` | Validate an issue body against the quality gate. | Read-only. |
| `forge-repair` | Repair thin Markdown into an executable issue shape. | Read-only. |
| `forge-interactive` | CLI-first guided issue shaping. | Creation requires `--write --confirm-create`. |
| `forge-reflect` | Reflect over local context and print candidate issues. | Read-only. |
| `forge-create` | Create a quality-gated tracker issue. | Requires explicit `--write`; can add to Project with `--add-to-project`. |

Examples:

```bash
cargo run -- forge-validate --title "Thin Forge issue" --file examples/fixtures/thin-issue.md
cargo run -- forge-reflect --context-file docs/dogfood-readiness.md --limit 1
cargo run -- forge-create --workflow examples/github-project-workflow.md --title "Follow-up title" --file /tmp/issue.md --add-to-project --write
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
```

## Merge Lane

| Command | Purpose | Boundary |
| --- | --- | --- |
| `merge-once` | Inspect one `Merging` issue, verify a single linked PR, and either merge or route blockers. | Live merge requires explicit `--write`; blocked PRs route to `Rework` or `Need Human Input`. |
| `land` | Compatibility alias for `merge-once`. | Same boundary as `merge-once`. |

Examples:

```bash
cargo run -- merge-once examples/github-project-workflow.md --dry-run
```

`merge-once` is separate from main implementation and review work. It should
only consume issues already in `Merging`.

## Live Dogfood Boundary

Use `examples/github-project-workflow.md` for Project #9 live reads and explicit
writes. Before running live write commands, confirm:

- the issue contract passes the Issue Quality Gate;
- the command includes `--write`;
- the target status is allowed for the current role;
- the workpad records evidence before state changes;
- the branch/PR belongs to exactly one issue.

Fixture success is useful rehearsal evidence, but it does not prove live GitHub
Project v2 readiness by itself.
