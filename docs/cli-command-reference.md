# CLI Command Reference

This reference describes the current `shea-symphony` command surface on `main`.
It is organized by operator task and safety boundary rather than by parser
order.

All live tracker mutations require explicit `--write`. Fixture-backed workflows
remain the preferred rehearsal path for local development. `doctor` can omit the
workflow path when `SHEA_SYMPHONY_WORKFLOW` is set, or when
`workflows/shea-symphony.md` exists in the current repo checkout.

For normal dogfood, Shea Symphony CLI is the authority for GitHub Project v2
workflow reads and mutations. Direct `gh issue view` / `gh pr view` is still
acceptable for raw issue or PR content when the CLI lacks the needed content
read, but Project status, Project fields, relationships, claim locks, workpads,
linked-PR handoff checks, and state transitions should go through the commands
in this reference. Manual Project UI or raw Project GraphQL changes are
break-glass recovery actions, not the standard path. See
`docs/github-access-policy.md` for the current raw GitHub inventory and
REST-first / GraphQL-required boundaries.

The canonical `workflows/shea-symphony.md` file is a workflow index/config. It
references lane-specific prompts in `workflows/prompts/` so Main, Review, and
Merge commands initialize with their own authority contracts. Older fixture
workflows may still use an inline prompt body.

## Read-Only Planning And Inspection

| Command | Purpose | Example |
| --- | --- | --- |
| `plan` | Default dispatch/status plan for a workflow. | `cargo run -- plan examples/dry-run-workflow.md` |
| `plan-dispatch` | Alias-style dispatch planning command. | `cargo run -- plan-dispatch examples/dry-run-workflow.md` |
| `dry-run` | Compatibility alias for planning output. | `cargo run -- dry-run examples/dry-run-workflow.md` |
| `status show` | Local runtime/session status snapshot; use `autopilot plan` for Project-backed planning. | `cargo run -- status show examples/dry-run-workflow.md` |
| `validate` | Validate workflow loading/configuration. | `cargo run -- validate examples/dry-run-workflow.md` |
| `validate-workflow` | Compatibility alias for `validate`. | `cargo run -- validate-workflow examples/dry-run-workflow.md` |
| `project state` | Diagnose whether the canonical Project read path is trustworthy. | `cargo run -- project state workflows/shea-symphony.md` |
| `project issue` | Read one issue's normalized Project state, fields, blockers, and linked PRs through Shea Symphony. | `cargo run -- project issue workflows/shea-symphony.md '#235' --json` |
| `project inspect` | Inspect one live issue's readiness facts without tracker mutation. | `cargo run -- project inspect workflows/shea-symphony.md '#235'` |
| `doctor` | Audit Project/workflow/runtime invariants. | `cargo run -- doctor` |
| `audit-project` | Compatibility alias for `doctor`. | `cargo run -- audit-project workflows/shea-symphony.md` |
| `profiles` | List configured/discovered backend profiles and validate repository runtime readiness in the current worktree. | `shea-symphony profiles /absolute/path/to/.shea/workflows/target.md` |
| `debug` | Read-only human report combining Project, doctor, smoke readiness, runtime/session, cleanup, and lane next-action signals. | `cargo run -- debug workflows/shea-symphony.md` |
| `autopilot plan` | Read-only Main/Review/Merge lane preflight with parked operator queues and foreground Autoloop readiness. | `cargo run -- autopilot plan workflows/shea-symphony.md` |
| `autopilot loop` | Bounded foreground all-lane supervisor tick that runs Main, Review, and Merge lane loops in order. | `cargo run -- autopilot loop workflows/shea-symphony.md --max-iterations 1 --write` |

The Autoloop plan (`autopilot plan`) is the mandatory planning bridge before `autopilot loop`. It
does not claim Project issues, launch Main/Review/Merge workers, start sessions,
write workpads, update runtime state, or mutate PRs. Its human output gives one
compact row for Main, Review, and Merge, plus parked `Human Review`,
`Need Human Input`, and dogfood/coordination queues. Its JSON output is the
stable preflight shape foreground automation should consume:

```bash
cargo run -- autopilot plan workflows/shea-symphony.md
cargo run -- autopilot plan workflows/shea-symphony.md --json
```

Readiness is explicit: `ready`, `idle_but_healthy`,
`blocked_by_doctor_or_canonical_checkout`, or
`blocked_by_ambiguous_lane_or_runtime_state`. Doctor blockers and canonical
checkout safety are blockers for write-mode Autoloop; historical Doctor
warnings remain visible evidence without automatically blocking the plan.

`autopilot loop` is a foreground command, not a daemon, background service, or
app-server. It currently requires `--max-iterations N` or `--once`; the flag
name is retained for compatibility, but the primary progress limit is completed
lane work units, not supervisor cycles. A single supervisor cycle may complete
Main, Review, and Merge work independently, and JSON events report both
`supervisor_cycle` and `completed_work_units` so consumers can distinguish
lifecycle from throughput. `--write` is still the mutation boundary. In write
mode, recover-first handling is enabled by default for interrupted Main and
Merge lane work; use `--no-recover` only for focused debugging. Per-lane
capacity uses `--main-max-concurrent`, `--review-max-concurrent`, and
`--merge-max-concurrent`.

Lane throughput is independent inside each foreground supervisor iteration. The
supervisor checks Main, Review, and Merge in that order, refreshes the plan
between lanes, and records one lane work-unit result for each lane. A slow,
blocked, idle, or busy lane remains visible in the status and result output, but
it is not a shared global iteration gate when another lane has ready work. A
global readiness blocker such as an unsafe canonical checkout, Doctor blocker,
or non-recoverable runtime ambiguity still blocks write-mode Autoloop before
lane mutation.

The iteration budget controls the supervisor lifetime, not the total number of
Main, Review, or Merge items that may complete. Lane-specific worker limits
control per-lane work-unit capacity for that iteration. For example,
`--max-iterations 1 --main-max-concurrent 2 --review-max-concurrent 1
--merge-max-concurrent 3` means one foreground supervisor pass may start or
recover up to two Main work units, one Review work unit, and three Merge work
units, subject to each lane's own claim locks and eligibility rules. Setting a
lane limit to `0` intentionally skips that lane for the iteration without
turning the other lanes off.

Use `--display tui` for a scannable foreground dashboard that shows Main,
Review, and Merge lane cards, parked operator queues, retry/backoff rows, and
recent loop events. `--json` remains a machine-output mode and cannot be
combined with `--display tui`. The TUI is rendered from an Autoloop dashboard
snapshot rather than from ad hoc terminal text, so a future Web UI should read
that shared snapshot shape instead of parsing terminal output.

Default resolution comes from workflow front matter unless a CLI override is
provided:

- `polling.interval_ms` -> `--poll-interval-ms`
- `main_lane.max_concurrent_agents` -> `--main-max-concurrent`
- `review_lane.max_concurrent_workers` -> `--review-max-concurrent`
- `merge_lane.max_concurrent_workers` -> `--merge-max-concurrent`

```bash
cargo run -- autopilot loop workflows/shea-symphony.md --max-iterations 1 --dry-run
cargo run -- autopilot loop workflows/shea-symphony.md --max-iterations 1 --write
cargo run -- autopilot loop workflows/shea-symphony.md --max-iterations 1 --dry-run --display tui
cargo run -- autopilot loop workflows/shea-symphony.md --once --dry-run --json
cargo run -- autopilot loop workflows/shea-symphony.md --max-iterations 3 --write --poll-interval-ms 30000
cargo run -- autopilot loop workflows/shea-symphony.md --max-iterations 1 --write --main-max-concurrent 2 --review-max-concurrent 1 --merge-max-concurrent 3
```

Normal dogfood should run `autopilot plan` first, then `autopilot loop --write`
from a clean canonical checkout on `main`. Use `main loop`, `review loop`, and
`merge loop` directly when the operator is intentionally debugging one lane,
replaying a bounded lane tick, or recovering a specific lane blocker.

`project state`, `autopilot plan`, `autopilot loop`, `main loop`, `review loop`,
`merge loop`, and the global Doctor scan use lightweight Project queue reads by
default. Those reads keep status, claim fields, assignee, priority, dependency,
and parent/subissue gate fields, but avoid issue bodies, comment/workpad streams,
and rich linked-PR hydration. Use `project issue '#<issue>' --json` or
`project inspect '#<issue>'` when an operator or lane needs the rich issue body,
workpad/timeline comments, linked PR readback, or detailed native topology
evidence for one issue.

Structured GitHub issue relationships have a small read/write surface under
`project relationship`. `list` and `verify` are read-only and accept `--dry-run`
for operator habit without requiring `--write`. Mutating commands require
`--write` and verify readback after adding the native relationship:

```bash
cargo run -- project relationship list workflows/shea-symphony.md '#123' --dry-run
cargo run -- project relationship verify workflows/shea-symphony.md '#123' --blocked-by '#122' --subissue '#124'
cargo run -- project relationship add-blocked-by workflows/shea-symphony.md '#123' '#122' --write
cargo run -- project relationship add-subissue workflows/shea-symphony.md '#120' '#123' --write
```

## Long-Running Command Progress

Live commands that wait longer than the centralized heartbeat threshold emit
compact progress lines to stderr, for example:

```text
progress wait=github_project_read elapsed=30s issue=#318 backend=gh next=load_issue
progress wait=review_backend elapsed=60s issue=#243 backend=gemini-cli artifact=/path/to/job.json next=waiting_for_child
```

These lines mean the command is still alive and waiting on the named backend or
child process. They do not change retry, timeout, routing, review, or merge
semantics. Timeout and backend failures still print their normal errors.

Heartbeat output is deliberately kept away from stdout, so JSON commands such as
`project issue --json`, `review status --json`, and `autopilot plan --json`
remain machine-readable. Lane loop heartbeats also append local
`progress_heartbeat` records to the configured `shea-symphony.jsonl` event log
when that command path already uses local runtime evidence.

The default threshold and repeat interval are 30 seconds. For UAT or local
simulation, set `SHEA_SYMPHONY_PROGRESS_HEARTBEAT_MS` to a smaller value; set it
to `0` to disable heartbeat output for that process. If a progress line keeps
repeating, use the `wait=`, `issue=`, `backend=`, `artifact=`, and `next=`
fields to choose the next diagnostic surface: `status show`, `review status`,
`doctor`, the referenced artifact path, or a recorded tmux attach command.

Doctor repair helpers:

```bash
cargo run -- doctor --interactive
cargo run -- doctor --auto-fix --dry-run
cargo run -- doctor --auto-fix --write
cargo run -- doctor repair 194
cargo run -- doctor repair 194 --move-need-human-input --write
cargo run -- doctor repair 194 --mark-pr-ready --confirm-handoff-ready --write
```

For operator-selected stuck states and `Need Human Input` triage, use the
repo-owned Doctor skill at `.agents/skills/shea-symphony-doctor/SKILL.md` with
the supporting spec in `docs/operator-doctor.md`. The skill is a read-first
diagnostic workflow that produces a structured `Doctor Triage Note`; it does
not replace the CLI repair commands or authorize automatic Project mutation.

`doctor repair ISSUE` is non-destructive by default. It prints safe,
uncertain, and dangerous actions for the issue using tracker state, Project
fields, runtime-state evidence, branch/PR hints, and doctor findings. The
`--move-need-human-input --write` path writes Doctor timeline evidence before changing
tracker state.
`--mark-pr-ready --confirm-handoff-ready --write` is an explicit operator repair
for `Agent Review` issues whose linked PR is still draft. It writes repair
evidence and runs `gh pr ready`; `doctor --auto-fix` never marks PRs ready.

## Main Implementation Runtime

| Command | Purpose | Boundary |
| --- | --- | --- |
| `main once` | Execute one selected issue through the configured backend. | Fixture-safe by default when the workflow has `tracker.fixture_path`. |
| `main loop` | Focused Main-lane poll/select/claim/run/reconcile/handoff in bounded or idle-loop modes. | Normal all-lane dogfood should enter through `autopilot plan` then `autopilot loop`; use `main loop` directly for Main-lane debugging, recovery, or deliberately bounded implementation-only work. Live write mode requires `--write` and a real main-agent backend; the canonical workflow uses Codex app-server by default; recover-first handling is enabled by default in `--write` mode and can be disabled with `--no-recover`; Agent Review handoff requires a verified Project-visible, ready, non-draft PR; native subissue PRs target the parent integration branch when topology evidence is present; parent issues with native subissues are skipped until every native subissue has Project status `Done`. |

Examples:

```bash
cargo run -- main once examples/dry-run-workflow.md
cargo run -- autopilot plan workflows/shea-symphony.md
cargo run -- autopilot loop workflows/shea-symphony.md --max-iterations 1 --write
cargo run -- main loop examples/dry-run-workflow.md --max-iterations 1 --dry-run
cargo run -- main loop examples/dry-run-workflow.md --max-iterations 1 --dry-run --display tui
cargo run -- main loop workflows/shea-symphony.md --max-iterations 1 --max-concurrent 2 --dry-run
cargo run -- main loop workflows/shea-symphony.md --max-iterations 1 --max-concurrent 3 --write
cargo run -- clean plan workflows/shea-symphony.md
cargo run -- clean audit workflows/shea-symphony.md
```

Use `--display tui` for an opt-in operator panel on focused `main loop`,
`project state`, and `doctor`. The default output stays line-oriented for logs
and scripts.

`main loop --max-concurrent N` is a supervised planning, claim-locking, and
runtime-slot boundary. Dry-run mode previews up to `N` eligible main-lane issues
after skipping items whose `Main Agent` Project field is already owned by
another worker. Write mode counts active Main runtime entries first, then claims
and starts up to the remaining capacity in the same bounded loop iteration. The
runtime-state file is backward-compatible with the old single active issue
shape, but can now persist multiple active Main worker entries without
overwriting another issue's session, workspace, retry, or transition evidence.
`main loop --write` uses recover-first handling by default for interrupted Main
runtime slots. It treats stalled runtime entries, missing session-registry
records, failed/stale app-server records, and unavailable tmux fallback panes as
recoverable capacity instead of blocking the lane, then restarts the same `In Progress` issue as a new attempt while
preserving the existing issue state, claim, workspace, dirty local changes, and
runtime evidence. Codex app-server session staleness defaults to 30 minutes and
can be configured with `codex.session_stale_after_ms`; stale app-server records
with process evidence are terminated before recovery resumes the recorded thread
with `Continue`. Codex app-server turn inactivity defaults to 5 minutes and can
be configured with `codex.stall_timeout_ms`; silent turns are terminated and
left retryable instead of waiting for the full turn timeout. When such an
app-server stall happens after a live issue worktree exists, `main loop --write`
tries the live handoff pipeline before terminal failure routing, so publishable
local work can still be verified, pushed, linked to the Project, and advanced
without an operator manually moving the issue out of `Need Human Input`. Use `--no-recover`
only for debugging or a deliberately
conservative operator pass. Recovery does not route through `Rework` and does
not advance to `Agent Review`; handoff still requires either a successful Main
result or the guarded app-server-stall live handoff salvage path.
`doctor` evaluates those runtime entries per issue so legitimate parallel Main
workers do not create false `runtime_active_issue_disagrees` warnings while
still surfacing missing, stale, or conflicting ownership. Planned claimable work
is reported separately from real active sessions; a Todo candidate is not
`running` until a backend session or runtime record exists. `main loop`, `review
loop`, and `merge once` print compact issue-scoped `Latest:` status bars for
real lane work; no-issue idle status and runtime telemetry stay in debug/JSON
surfaces instead of the default operator log stream.
Write-mode lane/control commands first run a guarded canonical checkout refresh
before the first tracker mutation. From a clean attached workflow git base
branch checkout (`git.base_branch`, default `main`), the CLI fetches the
upstream branch and fast-forwards with `git merge --ff-only` when that local
base branch is only behind. Output includes
`canonical_checkout_refresh=already_current`, `ff_only`, `would_ff_only`, or
`blocked`, followed by the normal `canonical_checkout ...` safety line.
Tracked dirty files, detached HEAD, non-base branches, missing upstreams,
unclassified untracked files, and non-fast-forward updates block the lane.
Recognized untracked runtime/log/prompt/evidence/draft artifacts are moved to
artifact quarantine with a warning before write-mode git or tracker mutation.
New lane claims are written as single-line `v=1` key/value audit pointers, for
example `v=1 lane=main actor=codex worker=codex-manual-main source=manual
issue=#244 run=... state=active thread=unknown registry=run/...`. Worker display
labels may contain spaces; the CLI stores those values with reversible quoting,
such as `worker="Codex Manual Main"`, and validates the rendered pointer before
writing Project fields. The Project field stores the compact pointer; the
session registry and workpad store the durable paths, logs, and handoff evidence
for the same `run=`. When a lane's authority ends, the owning field remains as
terminal audit evidence instead of being cleared: Main handoff writes
`state=done result=agent_review_handoff`, terminal Main blockers write
`state=failed result=<reason>`, Review pass/reject/inconclusive paths write a
terminal `Review Agent` claim with `result=passed|rejected|inconclusive|...`,
and Merge completion writes `state=done result=merged`.

Manual claim and session control are separate operations. Claim commands write
the lane claim Project field, create a matching `codex-app-manual` registry
record with status `recorded`, and do not change Project Status:

```bash
cargo run -- main claim workflows/shea-symphony.md '#265' --worker codex-manual-main --write
cargo run -- review claim workflows/shea-symphony.md '#265' --worker "Manual agy Review" --write
cargo run -- merge claim workflows/shea-symphony.md '#265' --worker codex-manual-merge --write
```

For parent tracking issues with native GitHub subissues, `main claim` uses the
same execution gate as `main loop`: it rejects `Todo` or `Rework` parents while
any native subissue has a missing or non-`Done` Project status after bounded
targeted child issue reads have had a chance to fill statuses omitted from the
parent read. This is independent from tracker blocker relationships so native
subissue changes cannot silently bypass parent dispatch safety.

Live write-mode claim, session, lane loop, review pass/reject, forge rework, and
workspace ensure commands refuse to run unless the canonical checkout is a clean
attached workflow git base branch checkout with a configured upstream. If that
local base branch is behind and can fast-forward, the CLI performs that
canonical-only `ff-only` refresh before continuing. It never refreshes issue
worktrees or PR branches in this path.

PR relationship verification is a lane invariant, not just evidence text. A PR
URL found in a workpad, issue comment, or local branch can help operators
identify the intended PR, but the issue must expose that PR through the
Project/issue linked-PR read surface before Main handoff, Review routing, or
Merge landing. If Shea Symphony cannot verify the relationship after a repair
attempt, it routes the issue to `Need Human Input` with the blocker preserved.
When Main handoff reuses an existing PR for the issue branch, the CLI preserves
the current PR body but appends a missing `Closes #<issue>` reference before
readback so GitHub can establish a native issue/PR relationship instead of
relying only on a timeline comment.

The canonical Main runtime is Codex app-server: `main_lane.backend: codex` plus
`codex.command: codex app-server -c 'service_tier="fast"'` and `codex.approval_policy: never`, matching
the current local app-server approval-policy schema. `autopilot loop` itself is
neither backend; it is the foreground CLI supervisor that invokes Main, Review,
and Merge lane commands. `main loop --write` records prompt, protocol, stderr,
normalized-event, runtime-state, and session-registry evidence for that
app-server turn before any `Agent Review` handoff. Selecting
`main_lane.backend: claude-code` or `merge_lane.agent_backend: claude-code`
uses the same lane result contract through Claude Code's non-interactive
stream-json CLI. Shea appends the protocol and resume flags to
`claude.command`, persists raw and normalized evidence, and fails closed unless
an initialized session produces an explicit successful result. The command or
wrapper retains model, authentication, gateway, environment, and permission
ownership; see `docs/claude-code-stream-json.md`. If `main_lane.backend: tmux`
is selected as explicit fallback/debug, the tmux path records its session name,
log path, workspace, branch, attach command, prompt artifact, actor, lane,
attempt, and running status in the durable session registry under the configured
artifact root. It still captures the pane before prompt injection and may
auto-advance the Codex workspace trust prompt only inside the configured Shea
Symphony issue worktree root. Set `SHEA_SYMPHONY_TMUX_AUTO_TRUST=0` to opt out;
a visible trust prompt or missing readiness then fails closed and preserves
attach/log evidence for inspection. The registry is runtime evidence only;
tracker state remains the issue lifecycle source of truth. On later ticks,
`autopilot loop --write` and `main loop --write` probe runtime/session evidence
before launching anything new. Completed sessions continue through
verification, PR publication, linked-PR readback, PR readiness, and
`Agent Review` handoff; active, waiting, unknown, or missing-registry sessions
are preserved without launching a duplicate Main Agent unless recover-first
handling is enabled and the session is classified as interrupted or unavailable.
Recover-first handling is enabled by default for `--write` and can be disabled
with `--no-recover`. If an operator overrides the workflow back to
`main_lane.backend: dry-run`, `main loop --write` and `autopilot loop --write`
exit non-zero before loading runtime state, creating worktrees, claiming Project
fields, or writing workpads. The dry-run preflight prints the selected Main
backend, backend source, command, approval policy, and session-registry path so
a bounded post-merge app-server smoke can stop before write mode if the selected
issue or backend is unexpected.

For manual lane recovery, first claim the lane and keep the printed `run=`.
Then `session start WORKFLOW ISSUE --lane main|review|merge --run RUN --write`
starts the configured lane runtime with the lane-specific prompt only after
confirming that the Project claim field already matches the issue, lane, and
run. Manual claim evidence is truthful non-runtime registry evidence; `session
start` never writes claim fields. Main and Merge-agent sessions default to Codex
app-server in the canonical workflow and may both select the shared Claude
stream-json backend, while Review session start remains the
supervised tmux fallback; set `main_lane.backend: tmux` or
`merge_lane.agent_backend: tmux` only for explicit fallback/debug. Clean
`merge once` / `merge loop` does not use this agent-session backend and remains
direct in-process CLI merge behavior. The rendered prompt includes the assigned
`run=` and registry pointer so the spawned agent can preserve that value in its
handoff evidence. `session list WORKFLOW` shows active registered sessions, and
`session attach WORKFLOW SESSION` prints the exact tmux attach command only for
tmux-backed sessions and does not join the terminal unless `--exec` is
provided. `status` and `status serve` include registered runtime session
summaries from the durable session registry with a backend label, so app-server,
tmux fallback, and manual Codex App evidence do not collapse into one tmux-only
surface. `doctor` flags stale, failed, orphaned, usage-limited, or
runtime/session mismatch cases with backend-aware recovery wording, while
`clean audit` classifies the registry, rendered prompts, app-server artifacts,
tmux fallback logs, and individual sessions without deleting them. Unknown
persisted registry status values are tolerated on read: they classify as
`unknown`, keep the raw status value in diagnostics, and are not migrated,
repaired, or rewritten by normal read-only commands. After a successful Main
handoff to `Agent Review`, Shea Symphony preserves the app-server artifacts or
tmux fallback log/attach command, reconciles matching Main session records to
`completed`, and clears matching active runtime state. This keeps `doctor` from
treating completed handoff evidence as active work while preserving recovery
evidence.

## Workspace Discovery

Use `workspace` when a lane needs to find or record the local git worktree for
an issue before starting review or merge repair. This command group is a safe
coordination surface for per-issue worktrees; it is not a generic checkout tool.
Discovery combines Project issue/PR hints, session registry records, canonical
Main Workpad evidence, timeline comments, and local `git worktree list --porcelain` output.

| Command | Purpose | Boundary |
| --- | --- | --- |
| `workspace list` | List discovered issue worktrees and inferred orphan hints. | Read-only Project-wide inventory; does not select or mutate workspaces. |
| `workspace show` | Show candidate worktrees for one issue. | Read-only lane preflight; multiple strong candidates require operator choice before local inspection. |
| `workspace adopt` | Record an operator-selected existing worktree in the issue workpad. | Validates the path is a git worktree for this repository and the branch matches issue/PR evidence; does not create a worktree or checkout a PR. |
| `workspace ensure` | Reuse or prepare a Review/Merge inspection worktree. | Reuse-first; creates only under the workflow workspace root, never switches the canonical checkout, and writes Workspace Evidence only with `--write`. |

Examples:

```bash
cargo run -- workspace list workflows/shea-symphony.md
cargo run -- workspace show workflows/shea-symphony.md '#253'
cargo run -- workspace adopt workflows/shea-symphony.md '#253' /tmp/shea-symphony-issue-253 --write
cargo run -- workspace ensure workflows/shea-symphony.md '#253' --dry-run
cargo run -- workspace ensure workflows/shea-symphony.md '#253' --pr 254 --write
```

Review lane uses discovered worktrees for read-only inspection by default.
Merge lane should prefer the canonical Main PR worktree/branch for merge-lane
repair instead of creating a replacement workspace. If no suitable candidate is
available, `workspace ensure` prepares the inspection worktree under the
configured workspace root and records durable `### Workspace Evidence` in the
issue workpad. `workspace adopt` is only
for an operator-selected existing worktree; it must not be used as a shortcut
for `gh pr checkout` in the canonical checkout. `doctor` warns when multiple
strong candidates exist for one active issue.

For native parent/subissue flows, `doctor` also checks the read-only integration
branch topology from GitHub native parent/subissue links plus Shea-owned branch
and merge evidence. It reports blockers for subissue PRs targeting `main`,
missing or ambiguous parent integration branch evidence, `Done` subissues
without parent-branch merge evidence, and parent `Human Review` before native
subissues are complete.

## Tracker Writes

These commands can mutate live tracker state and require `--write`.

GitHub Project v2 field writes are REST-first where GitHub supports the field
kind and the Project read exposes REST item and field IDs. Status and lane claim
text fields use the REST item update path first, then fall back to the existing
GraphQL mutations when REST capability data is missing. Project metadata and
field IDs are cached only within the current CLI process and refreshed once when
a lookup appears stale.

| Command | Purpose | Boundary |
| --- | --- | --- |
| `project set-state` | Move one issue to a normalized workflow state. | Refuses `Human Review` from the main implementation role. |
| `project workpad` | Upsert the canonical Main Agent Workpad marker comment. | Use for Main implementation evidence, including Main-lane `Rework` implementation rounds. Repeated canonical workpad writes replace the prior canonical workpad entry instead of creating multiple top-level `Shea Symphony Workpad` blocks. Append-only `Shea Symphony Rework Run` comments explain why Rework was triggered; Review, Merge, Human Review, and Doctor evidence should remain append-only timeline comments created by their lane commands. |
| `project timeline-comment` | Append one standalone issue timeline comment from a Markdown file. | Use for lane evidence that must not overwrite the Main Agent Workpad, especially Human Review decision notes or operator-authored Doctor/repair evidence. |
| `project link-pr` | Verify GitHub-native issue-to-PR linkage, with fallback comments treated as diagnostics only. | First checks native linked-PR readback and succeeds only when GitHub exposes the PR through its first-class linked PR surface. A fallback comment/workpad PR URL can help diagnose the intended PR, but it is not accepted as native linkage success. |
| `create-follow-up` | Create a follow-up issue from a body file. | Lower-level creation path; prefer `forge create` for quality-gated issues. |
| `project add` | Add an existing GitHub issue node to the configured Project. | Initializes configured Project status where supported. |

Transient GitHub REST or GraphQL failures after a write are reconciled with a
readback before the command fails. For claim fields, workpads, timeline
comments, Project status, merge completion, and issue closure the CLI prints
`tracker_recovery action=recovered ... next=continue` when readback proves the
mutation landed. If readback cannot prove the outcome, the command fails with
`recoverable_tracker_mutation_uncertain` and a `next=` hint; rerun the same
lane command after waiting or read back the issue through `project issue`.
Append-only lane evidence carries a hidden recovery marker, so rerunning the
same lane/run skips already-recorded evidence instead of posting a duplicate
large comment. Already-applied recovery checks are quiet by default because
they are idempotence confirmations rather than operator actions.

Examples:

```bash
cargo run -- project set-state workflows/shea-symphony.md '#123' need_to_clarify --write
cargo run -- project workpad workflows/shea-symphony.md '#123' /tmp/workpad.md --write
cargo run -- project timeline-comment workflows/shea-symphony.md '#123' /tmp/human-review-note.md --write
```

## Clean Lane

`clean` owns local cleanup and persistence-audit concerns. `doctor` remains
focused on tracker/runtime health, stuck workflow states, PR/review/merge
invariants, and repair evidence.

| Command | Purpose | Boundary |
| --- | --- | --- |
| `clean plan` | Read-only cleanup planning command. | Reports terminal clean worktrees that are cleanup candidates; never deletes. |
| `clean audit` | Classify configured artifact/workspace residue by persistence action. | Read-only; categories include `promote_to_repo`, `attach_to_tracker`, `safe_to_keep`, `cleanup_candidate`, `needs_human_decision`, and canonical checkout quarantine. |

Examples:

```bash
cargo run -- clean plan workflows/shea-symphony.md
cargo run -- clean audit workflows/shea-symphony.md
```

## Issue Readiness And Contract Validation

| Command | Purpose | Boundary |
| --- | --- | --- |
| `forge validate` | Validate issue contract quality for a draft body or existing issue. | Read-only; `Todo` uses the full Issue Quality Gate, `Backlog` uses the lighter seed gate. |
| `project inspect` | Read live Project readiness, blockers, linked PR evidence, and dispatchability for one issue. | Read-only; does not claim, route, write workpads, or change status. |

Examples:

```bash
cargo run -- forge validate --workflow workflows/shea-symphony.md --issue '#123'
cargo run -- project inspect workflows/shea-symphony.md '#123'
```

## Issue Forge

| Command | Purpose | Boundary |
| --- | --- | --- |
| `forge validate` | Validate a body file, an existing issue, or candidate title/body content against live issue context for `Backlog` or `Todo`. | Read-only; `Todo` uses the full Issue Quality Gate, `Backlog` uses the lighter seed gate; output separates candidate contract gaps from live context gaps. |
| `forge create` | Create a Project-backed issue in `Backlog` or `Todo`. | Dry-run by default unless `--write` is supplied; initializes the Project item to the requested status and verifies readback; write success keeps `issue_id` and adds readback issue number, URL, and `project_status` when the tracker provides them; live `Todo` creation requires `--assignee`; `--blocked-by` and `--parent` declare native relationship writes that must be read back before final `Todo` status. |
| `forge promote` | Promote one existing Backlog issue in place by editing title/body, writing a structured Promotion Note comment, then moving it to `Todo`. | Dry-run by default unless `--write` is supplied; requires structured note inputs, keeps the `Todo` status mutation last, reports the checkpoint where any failure stopped, and can satisfy blocker/parent gates through `--blocked-by` / `--parent` relationship plans. |
| `forge rework` | Revise one live `Human Review` issue into an explicit `Rework` contract. | Dry-run by default unless `--write` is supplied; requires a replacement title/body, evidence file, and operator confirmation; rejects active lane claims and keeps the `Rework` status mutation last. |

Examples:

```bash
cargo run -- forge validate --workflow workflows/shea-symphony.md --status Backlog --title "Backlog seed" --body-file /tmp/issue.md
cargo run -- forge validate --workflow workflows/shea-symphony.md --status Todo --title "Executable issue" --body-file /tmp/issue.md
cargo run -- forge validate --workflow workflows/shea-symphony.md --issue '#293' --status Todo --title "Candidate promoted title" --body-file /tmp/candidate.md
cargo run -- forge create --workflow workflows/shea-symphony.md --status Backlog --title "Backlog: follow-up title" --body-file /tmp/issue.md --dry-run
cargo run -- forge create --workflow workflows/shea-symphony.md --status Todo --title "Follow-up title" --body-file /tmp/issue.md --assignee Alive24 --write
cargo run -- forge create --workflow workflows/shea-symphony.md --status Todo --title "Blocked follow-up title" --body-file /tmp/issue.md --assignee Alive24 --blocked-by '#122' --dry-run
cargo run -- forge promote '#241' --workflow workflows/shea-symphony.md --title "Executable title" --body-file /tmp/issue.md --operator-confirmation "promote it" --decision "Use the CLI-owned promotion note template." --scope-change "Backlog seed is now an executable Todo issue." --dependency-context "Dependencies: none; related context is non-blocking." --readback-summary "Operator confirmed the dry-run preview before write." --dry-run
cargo run -- forge promote '#241' --workflow workflows/shea-symphony.md --title "Executable title" --body-file /tmp/issue.md --operator-confirmation "promote it" --decision "Use the CLI-owned promotion note template." --scope-change "Backlog seed is now an executable Todo issue." --dependency-context "Blocked by #122 until the prerequisite lands." --blocked-by '#122' --readback-summary "Operator confirmed the dry-run preview before write." --dry-run
cargo run -- forge promote '#241' --workflow examples/promote-fixture-workflow.md --title "Harden Issue Forge Reflect promotion fixture" --body-file examples/fixtures/promoted-issue.md --operator-confirmation "promote it" --decision "Keep the promotion in place." --scope-change "Backlog seed becomes an executable Todo issue." --dependency-context "Dependencies: none." --readback-summary "Dry-run preview verified before write." --dry-run
cargo run -- forge rework '#282' --workflow workflows/shea-symphony.md --title "Rework: revised execution contract" --body-file /tmp/rework-body.md --evidence-file /tmp/rework-evidence.md --operator-confirmation "route Human Review back to Rework" --dry-run
```

Successful `forge create --write` output is a single parseable line. It keeps
the machine-facing tracker node id while exposing live tracker readback metadata
for operator lookup:

```text
forge_create=ok issue_id=I_kw... issue=#305 url=https://github.com/Alive24/shea-symphony/issues/305 status=Backlog project_status=Backlog project_fields=0
```

Dry-run output does not invent issue numbers or URLs.

Use `forge validate --issue '#123'` without overrides to validate the current
live issue body. Add `--title` plus `--body` or `--body-file` when validating a
candidate replacement contract against the live issue's assignee and Project
context. `forge promote --dry-run` uses the same validation output categories,
then adds the promotion-note preview and promotion-specific checks.

`forge promote` owns the Promotion Note requirement. The command refuses missing
or empty `--operator-confirmation`, `--decision`, `--scope-change`, and
`--dependency-context` values, and accepts repeatable `--readback-summary`
values for operator-supplied verification notes. In write mode, it edits the
issue body/title, verifies that content readback, writes the Promotion Note, and
only then moves the Project status from `Backlog` to `Todo`; after that final
mutation it performs read-only status readback. On write success, the comment
uses this short Markdown shape:

When a Todo candidate names blocking dependencies in `## Issue Setup`
`Dependencies:` or in a standalone `## Dependencies` section, the quality gate
requires either `Dependencies: None` or a structured relationship plan. Use
repeatable `--blocked-by '#<issue>'` for native GitHub blocked-by relationships
and `--parent '#<issue>'` when the candidate should become a native subissue.
`forge create --write` stages relationship-backed Todo candidates in `Backlog`,
adds and verifies relationships, then performs the final Todo status mutation.

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

`forge rework` owns the Human Review -> Rework contract-revision path. The
interactive decision still belongs in the Issue Forge skill layer; the CLI
accepts prepared files and confirmation, verifies the source issue is in
`Human Review`, rejects active `Main Agent`, `Review Agent`, or `Merging Agent`
claims with a diagnostic workpad, preserves terminal `state=done` or other
terminal claim values as audit pointers, replaces the title/body, writes Rework
revision evidence to the workpad, and only then sets Project status to
`Rework`. It does not require a linked PR or local worktree, and it does not
create or recover either one; the next Main Agent owns that repair setup after
claiming `Rework`.

## Review Agent Lane

The main implementation agent must never set `Human Review`. Review commands
represent the independent review lane and must record evidence before status
changes.

| Command | Purpose | Boundary |
| --- | --- | --- |
| `review fake` | Fixture/fake review transition helper. | Local testing path. |
| `review once` | Run one configured review backend for one issue. | Direct backend command for one issue. |
| `review loop` | Bounded review worker selection/reconciliation. | The canonical `agy-cli` backend runs `agy --print --mode plan` headlessly. `codex-app-server` and `claude-code` run independent fresh, read-only/non-interactive sessions with schema-validated output and fail-closed protocol handling. The legacy `gemini-cli` backend remains available. Every backend reuses the existing claims, worker pool, artifacts, ledgers, and routing. |
| `review status` | Read review-loop and review-runner status from local ledgers, runtime/session registry, and Project claim cross-checks. | Read-only; never claims, repairs, retries, kills jobs, writes workpads, or changes Project state. |
| `review claim` | Claim one `Agent Review` item's `Review Agent` text field for manual/operator review. | Requires `--worker` and `--write`; refuses non-`Agent Review` issues and writes a structured, round-trip-validated claim pointer. |
| `review pass` | Record manual independent review pass evidence and route to the correct next state. | Requires `--write`, a durable evidence file containing the exact current `Review Agent` claim, and preserves the field as terminal pass evidence. Ordinary issues and parent final issues route to `Human Review`; routine native subissues route directly to `Merging` unless they record `Subissue Human Review Exception: <reason>`. |
| `review reject` | Record failed/inconclusive manual review evidence and route to `Agent Review`, `Rework`, or `Need Human Input`. | Refuses `Human Review`, requires exact claim evidence, and preserves the field as terminal reject/failed evidence. |
| `review session` | Hidden legacy review session alias. | Does not write the `Review Agent` claim; use `review claim` or `review loop` for claim ownership. |
| `review freshness` | Record/inspect review freshness evidence. | Used around merging/rework conflict repair. |
| `review-clear-claim` | Clear one issue's `Review Agent` claim through the tracker adapter. | Requires `--write`; use after terminal manual review routing. |
| `session start` | Start the configured local runtime for a selected lane and `run`. | Manual recovery path; validates an existing lane claim, selects the lane-specific command/backend, and does not write Project claim fields. Main and Merge agent sessions default to Codex app-server in the canonical workflow; Review remains the supervised tmux fallback. |
| `session list` | List active Shea Symphony tmux sessions by configured prefix. | Read-only operator summary. |
| `session attach` | Print or execute the tmux attach command for one session. | Defaults to printing the command; `--exec` enters tmux. |

Review backend implementations own backend-specific command previews, prelaunch
diagnostics, stdout parsing, and artifact shaping. The current canonical
workflow selects `review_lane.backend: agy-cli` with `agy_command` and
`agy_model`. Legacy `gemini-cli` workflows can still use `gemini_command`,
`gemini_model`, and `gemini_allowed_tools`.

Codex Review uses `review_lane.backend: codex-app-server`. Its optional
`codex_command` overrides and otherwise falls back to `codex.command`;
`codex_approval_policy` must be `never`, `codex_thread_sandbox` must be
`read-only`/`readOnly`, and an optional `codex_turn_sandbox_policy` must have
type `readOnly`. A fresh Review job never resumes Main or Merge state.

Claude Review uses `review_lane.backend: claude-code`. Its optional
`claude_command` overrides and otherwise falls back to `claude.command`. Shea
appends the same `-p --input-format stream-json --output-format stream-json
--verbose` protocol flags used by Main and Merge. The configured command owns
model, authentication, gateway, environment, and read-only permission policy.
Every new Review job starts fresh; only one interrupted attempt may resume that
same job's recorded session. Missing or invalid structured output and any
workspace mutation fail closed.

Example:

```bash
cargo run -- review loop examples/review-fixture-workflow.md --max-iterations 1 --dry-run
cargo run -- review loop workflows/shea-symphony.md --max-iterations 1 --write
cargo run -- review status workflows/shea-symphony.md
cargo run -- review status workflows/shea-symphony.md --issue '#226' --recent 3 --verbose
cargo run -- review status workflows/shea-symphony.md --json
cargo run -- review claim workflows/shea-symphony.md '#226' --worker "Manual agy Review" --write
cargo run -- session start workflows/shea-symphony.md '#226' --lane review --run <RUN_ID> --write
cargo run -- session list workflows/shea-symphony.md
cargo run -- review pass workflows/shea-symphony.md '#226' --evidence-file /tmp/review-evidence.md --write
cargo run -- review reject workflows/shea-symphony.md '#226' --evidence-file /tmp/review-evidence.md --target-state rework --write
```

Manual review evidence files must include the exact structured `Review Agent`
claim printed by `review claim` or recorded by `review loop`. Terminal routing
updates that same field to an audit pointer such as `state=done result=passed`,
`state=done result=rejected`, `state=failed result=inconclusive`, or
`state=failed result=blocked`; it does not clear the field.

Backend-backed `review loop` distinguishes recoverable backend health from
operator-action blockers. Quota, rate-limit, and resource-exhausted responses
wait and retry when the loop is allowed to continue; transient capacity,
network, timeout, or 5xx failures retry with bounded backoff while keeping the
issue in `Agent Review`. Command, auth, model, policy, or tool-permission
configuration failures route to `Need Human Input`. Repeated same-cause backend
failures append compact repeat evidence instead of duplicating full logs.

## Repository-Owned Skills

All first-party Shea and HALO Skills are authored once under `.agents/skills`.
The standard Skills CLI can discover that tree and vendor selected Skills into
another repository:

```bash
npx skills add https://github.com/Alive24/shea-symphony/tree/main/.agents/skills --list
npx skills add https://github.com/Alive24/shea-symphony/tree/main/.agents/skills \
  --skill shea-symphony-doctor
```

Vendoring is an initial copy operation, not a managed Shea package lifecycle.
Afterward the target repository owns those files and may customize them. Shea's
CLI and Doctor do not install, update, remove, restore, version-check, or compare
vendored Skill text with upstream.

The canonical inventory includes Issue Forge, Issue Forge Reflect, Issue Forge
Dream, Runtime Onboarding, Manual Main, Manual Review, Human Review, Manual
Merge, Doctor, and the HALO research seed. Human
Review is an operator-owned briefing and UAT decision skill: it records a
structured decision note and routes to `Merging`, `Rework`, or
`Need Human Input` only after explicit operator confirmation. `forge reflect`
and `forge dream` remain skill behaviors, not Shea Symphony CLI subcommands.
`forge create`, `forge promote`, `forge rework`, and `forge validate` remain
deterministic CLI executor surfaces.

## Issue Forge Dream

Issue Forge Dream is a Codex/Gemini skill workflow for slow, deep backlog
mining. It reads broader Shea Symphony context, writes bounded advisory logs,
runs a lightweight Gemini review by default when available, and creates
evidence-backed `Backlog` seeds unless the operator asks for report-only mode.

Dream writes repo-owned logs under `docs/dream-log/`:

- `docs/dream-log/INDEX.md` is the compact global entrypoint.
- Each run directory uses `docs/dream-log/YYYY-MM-DD-<run-count>-<slug>/`.
- `RUN.md` records the source inventory, created backlog mapping, sleep-enough
  judgment, Gemini review status, and next useful theme.
- `topic-*.md` records bounded topic triage with evidence anchors, coverage
  checks, promotion path, and Dream confidence.
- `gemini-review.md` records the lightweight review summary or unavailable
  reason.
- `created-backlog.md` is optional when several seeds are created.

Dream-created Backlog seeds should include evidence anchors, existing coverage
checked, promotion guidance, and Dream confidence. Low-confidence candidates
stay Watchlist or very light Backlog. Dream never creates `Todo` issues
directly and Dream Logs are not execution authority for Main, Review, Merge, or
Doctor lanes.

## Merge Lane

| Command | Purpose | Boundary |
| --- | --- | --- |
| `merge once` | Inspect one `Merging` issue, verify a single linked PR, and either merge, safely refresh a stale branch, attempt safe conflict repair, or route blockers. | Live merge requires explicit `--write`; fixture workflows synthesize merge or conflict-repair command evidence without touching GitHub. Native subissues expect the parent integration branch as the PR base; parent final PRs expect the configured workflow git base branch (`git.base_branch`, default `main`). `BEHIND` PRs are updated with `gh pr update-branch` and left in `Merging` for retry, transient `UNKNOWN` mergeability stays in `Merging`, `DIRTY` PRs first try direct clean local PR-worktree repair, then use the configured merge-agent backend for content conflicts in a trusted clean PR worktree. Interrupted conflict-repair merge states are aborted before retry. Successful repair and retryable backend or verification failures stay in `Merging`; only semantic uncertainty, unsafe or untrusted preconditions, untracked-file residue, push failures, or failing checks route to `Need Human Input` with a concrete question instead of defaulting to `Rework`. |
| `merge loop` | Repeat guarded merge ticks for an explicit bounded iteration count. | Requires `--max-iterations` or `--once`; `--max-concurrent N` processes up to `N` merge slots while respecting `Merging Agent` claim fields; recover-first handling is enabled by default in `--write` mode and can be disabled with `--no-recover`. |

Examples:

```bash
cargo run -- merge once workflows/shea-symphony.md --dry-run
cargo run -- merge loop examples/merge-fixture-workflow.md --max-iterations 1 --write
cargo run -- merge loop examples/merge-conflict-repair-fixture-workflow.md --max-iterations 1 --write
cargo run -- merge loop workflows/shea-symphony.md --max-iterations 2 --max-concurrent 2 --write
```

`merge once` is separate from main implementation and review work. It should
only consume issues already in `Merging`. `Rework` remains a Main/Review repair
lane unless an operator explicitly chooses a historical merge-lane recovery
path.
`merge once` is the direct single-tick primitive; `merge loop` and `autopilot
loop` may call that primitive internally, but their operator-visible output is
reported at the `merge_loop` / `autopilot` layer rather than leaking
`merge_once*` implementation names.
`merge loop --write` uses recover-first handling by default for interrupted
in-process merge runs. Because merge work has no long-lived tmux session to
probe, recovery is tracker-first: it only adopts structured active merge claims
created by loop or goal sources, leaves manual claims alone, keeps the issue in
`Merging` for safe stale-base updates or merge-lane repairs, and falls back to
normal unclaimed merge selection after recoverable claims have been handled.
Use `--no-recover` only for debugging or a deliberately conservative operator
pass.

## Live Dogfood Boundary

Use `workflows/shea-symphony.md` for Project #9 live reads and explicit
writes. Before running live write commands, confirm:

- the issue contract passes the Issue Quality Gate;
- the command includes `--write`;
- the target status is allowed for the current role;
- the workpad records evidence before state changes;
- the branch/PR belongs to exactly one issue.

Fixture success is useful rehearsal evidence, but it does not prove live GitHub
Project v2 readiness by itself.
