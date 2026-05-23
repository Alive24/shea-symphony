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
acceptable for raw issue or PR content when the CLI lacks the needed content
read, but Project status, Project fields, relationships, claim locks, workpads,
linked-PR handoff checks, and state transitions should go through the commands
in this reference. Manual Project UI or raw Project GraphQL changes are
break-glass recovery actions, not the standard path. See
`docs/github-access-policy.md` for the current raw GitHub inventory and
REST-first / GraphQL-required boundaries.

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
| `status show` | Operator status view for planning output. | `cargo run -- status show examples/dry-run-workflow.md` |
| `validate` | Validate workflow loading/configuration. | `cargo run -- validate examples/dry-run-workflow.md` |
| `validate-workflow` | Compatibility alias for `validate`. | `cargo run -- validate-workflow examples/dry-run-workflow.md` |
| `project state` | Diagnose whether the canonical Project read path is trustworthy. | `cargo run -- project state workflows/jade-symphony.md` |
| `project issue` | Read one issue's normalized Project state, fields, blockers, and linked PRs through Jade Symphony. | `cargo run -- project issue workflows/jade-symphony.md '#235' --json` |
| `project inspect` | Inspect one live issue's readiness facts without tracker mutation. | `cargo run -- project inspect workflows/jade-symphony.md '#235'` |
| `doctor` | Audit Project/workflow/runtime invariants. | `cargo run -- doctor` |
| `audit-project` | Compatibility alias for `doctor`. | `cargo run -- audit-project workflows/jade-symphony.md` |
| `skills status` | Read-only per-repo skill readiness matrix across source suite, Codex, Gemini, metadata, links, and optional session input. | `cargo run -- skills status workflows/jade-symphony.md` |
| `profiles` | List configured/discovered execution profiles. | `cargo run -- profiles examples/cockpit-profiles-workflow.md` |
| `debug` | Read-only human report combining Project, doctor, smoke readiness, runtime/session, cleanup, and lane next-action signals. | `cargo run -- debug workflows/jade-symphony.md` |
| `autopilot plan` | Read-only Main/Review/Merge lane preflight with parked operator queues and future write-mode readiness. | `cargo run -- autopilot plan workflows/jade-symphony.md` |

`autopilot plan` is the mandatory planning bridge before any future all-lane
write-mode autopilot. It does not claim Project issues, launch Main/Review/Merge
workers, start tmux sessions, write workpads, update runtime state, or mutate
PRs. Its human output gives one compact row for Main, Review, and Merge, plus
parked `Human Review`, `Need Human Input`, and dogfood/coordination queues. Its
JSON output is the stable preflight shape future automation should consume:

```bash
cargo run -- autopilot plan workflows/jade-symphony.md
cargo run -- autopilot plan workflows/jade-symphony.md --json
```

Readiness is explicit: `ready`, `idle_but_healthy`,
`blocked_by_doctor_or_canonical_checkout`, or
`blocked_by_ambiguous_lane_or_runtime_state`. Doctor blockers and canonical
checkout safety are blockers for future write-mode autopilot; historical Doctor
warnings remain visible evidence without automatically blocking the plan.

`project state`, `main loop`, `review loop`, `merge loop`, and the global
Doctor scan use lightweight Project queue reads by default. Those reads keep
status, claim fields, assignee, priority, dependency, and parent/subissue gate
fields, but avoid issue bodies, comment/workpad streams, and rich linked-PR
hydration. Use `project issue '#<issue>' --json` or `project inspect '#<issue>'`
when an operator or lane needs the rich issue body, workpad/timeline comments,
linked PR readback, or detailed native topology evidence for one issue.

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
`progress_heartbeat` records to the configured `jade-symphony.jsonl` event log
when that command path already uses local runtime evidence.

The default threshold and repeat interval are 30 seconds. For UAT or local
simulation, set `JADE_SYMPHONY_PROGRESS_HEARTBEAT_MS` to a smaller value; set it
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
repo-owned Doctor skill at `.codex/skills/jade-symphony-doctor/SKILL.md` with
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

Skill readiness is diagnostic-first and read-only:

```bash
cargo run -- skills status workflows/jade-symphony.md
cargo run -- skills status workflows/jade-symphony.md --json
cargo run -- skills status workflows/jade-symphony.md --suite-path skills/jade-symphony/suite
cargo run -- skills status workflows/jade-symphony.md --session-skills-file /path/to/session-skills.txt
cargo run -- skills status workflows/jade-symphony.md --require-gemini
```

The command discovers expected skills from `--suite-path`,
`JADE_SYMPHONY_SKILL_SUITE`, the current repo `skills/jade-symphony/suite`, or
installed-only mode if no source suite exists. It inspects Codex local skills,
Gemini local skills when configured or discoverable, rendered metadata drift,
broken symlinks, file-shaped aliases, missing `SKILL.md`, and optional
current-session visibility. Without `--session-skills` or
`--session-skills-file`, current-session visibility is `unknown` and is not a
failure. Gemini absence is a blocker only when `--require-gemini` is used.

## Main Implementation Runtime

| Command | Purpose | Boundary |
| --- | --- | --- |
| `main once` | Execute one selected issue through the configured backend. | Fixture-safe by default when the workflow has `tracker.fixture_path`. |
| `main loop` | Poll/select/claim/run/reconcile/handoff in bounded or idle-loop modes. | Live write mode requires `--write` and a real main-agent backend; recover-first handling is enabled by default in `--write` mode and can be disabled with `--no-recover`; tmux sessions stay active until a later loop observes terminal evidence; Agent Review handoff requires a verified Project-visible, ready, non-draft PR; native subissue PRs target the parent integration branch when topology evidence is present; parent issues with native subissues are skipped until every native subissue has Project status `Done`. |

Examples:

```bash
cargo run -- main once examples/dry-run-workflow.md
cargo run -- main loop examples/dry-run-workflow.md --max-iterations 1 --dry-run
cargo run -- main loop examples/dry-run-workflow.md --max-iterations 1 --dry-run --display tui
cargo run -- main loop workflows/jade-symphony.md --max-iterations 1 --max-concurrent 2 --dry-run
cargo run -- main loop workflows/jade-symphony.md --max-iterations 1 --max-concurrent 3 --write
cargo run -- clean plan workflows/jade-symphony.md
cargo run -- clean audit workflows/jade-symphony.md
```

Use `--display tui` for an opt-in operator panel on `main loop`, `project state`,
and `doctor`. The default output stays line-oriented for logs and scripts.

`main loop --max-concurrent N` is a supervised planning, claim-locking, and
runtime-slot boundary. Dry-run mode previews up to `N` eligible main-lane issues
after skipping items whose `Main Agent` Project field is already owned by
another worker. Write mode counts active Main runtime entries first, then claims
and starts up to the remaining capacity in the same bounded loop iteration. The
runtime-state file is backward-compatible with the old single active issue
shape, but can now persist multiple active Main worker entries without
overwriting another issue's session, workspace, retry, or transition evidence.
`main loop --write` uses recover-first handling by default for interrupted Main
tmux runtime slots. It treats stalled runtime entries, missing session-registry
records, and unavailable tmux panes as recoverable capacity instead of blocking
the lane, then restarts the same `In Progress` issue as a new attempt while
preserving the existing issue state, claim, workspace, dirty local changes, and
runtime evidence. Use `--no-recover` only for debugging or a deliberately
conservative operator pass. Recovery does not route through `Rework` and does
not advance to `Agent Review`; normal handoff still requires a later successful
Main result.
`doctor` evaluates those runtime entries per issue so legitimate parallel Main
workers do not create false `runtime_active_issue_disagrees` warnings while
still surfacing missing, stale, or conflicting ownership. Planned claimable work
is reported separately from real active sessions; a Todo candidate is not
`running` until a backend session or runtime record exists. `main loop`, `review
loop`, and `merge once` print compact `Latest:` status bars in addition to their
detailed line logs.
Write-mode lane/control commands first run a guarded canonical checkout refresh
before the first tracker mutation. From a clean attached `main` checkout, the
CLI fetches the upstream branch and fast-forwards with `git merge --ff-only`
when local `main` is only behind. Output includes
`canonical_checkout_refresh=already_current`, `ff_only`, `would_ff_only`, or
`blocked`, followed by the normal `canonical_checkout ...` safety line.
Tracked dirty files, detached HEAD, non-`main` branches, missing upstreams,
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
for the same `run=`.

Manual claim and session control are separate operations. Claim commands write
the lane claim Project field, create a matching `codex-app-manual` registry
record with status `recorded`, and do not change Project Status:

```bash
cargo run -- main claim workflows/jade-symphony.md '#265' --worker codex-manual-main --write
cargo run -- review claim workflows/jade-symphony.md '#265' --worker "Manual Gemini Review" --write
cargo run -- merge claim workflows/jade-symphony.md '#265' --worker codex-manual-merge --write
```

For parent tracking issues with native GitHub subissues, `main claim` uses the
same execution gate as `main loop`: it rejects `Todo` or `Rework` parents while
any native subissue has a missing or non-`Done` Project status after bounded
targeted child issue reads have had a chance to fill statuses omitted from the
parent read. This is independent from tracker blocker relationships so native
subissue changes cannot silently bypass parent dispatch safety.

Live write-mode claim, session, lane loop, review pass/reject, forge rework, and
workspace ensure commands refuse to run unless the canonical checkout is a clean
attached `main` checkout with a configured upstream. If local `main` is behind
and can fast-forward, the CLI performs that canonical-only `ff-only` refresh
before continuing. It never refreshes issue worktrees or PR branches in this
path.

PR relationship verification is a lane invariant, not just evidence text. A PR
URL found in a workpad, issue comment, or local branch can help operators
identify the intended PR, but the issue must expose that PR through the
Project/issue linked-PR read surface before Main handoff, Review routing, or
Merge landing. If Jade Symphony cannot verify the relationship after a repair
attempt, it routes the issue to `Need Human Input` with the blocker preserved.
When Main handoff reuses an existing PR for the issue branch, the CLI preserves
the current PR body but appends a missing `Closes #<issue>` reference before
readback so GitHub can establish a native issue/PR relationship instead of
relying only on a timeline comment.

The canonical `workflows/jade-symphony.md` file uses the local `tmux` main-agent
backend. A launched tmux session records its session name, log path, workspace,
branch, attach command, prompt artifact, actor, lane, attempt, and running
status in a durable session registry under the configured artifact root. The
registry is terminal-session evidence only; tracker state remains the issue
lifecycle source of truth. The issue stays in the active main lane until later
completion evidence satisfies the existing handoff rules. On later ticks,
`main loop --write` probes the runtime state's recorded session through the
session registry plus bounded tmux pane/log evidence before launching anything
new. Completed sessions continue through verification, PR publication,
linked-PR readback, PR readiness, and `Agent Review` handoff; active, waiting,
unknown, or missing-registry sessions are preserved without launching a
duplicate Main Agent unless recover-first handling is enabled and the session is
classified as interrupted or unavailable. Recover-first handling is enabled by
default for `--write` and can be disabled with `--no-recover`. If an operator
overrides the workflow back to `main_lane.backend: dry-run`, `main loop --write`
exits non-zero before loading runtime state, creating worktrees, claiming
Project fields, or writing workpads.

When the tmux agent command is Codex, `main loop --write` captures the pane before
prompt injection. The default behavior auto-advances the Codex workspace trust
prompt only inside the configured Jade Symphony issue worktree root, then injects the
rendered prompt after a ready viewport is observed. Set
`JADE_SYMPHONY_TMUX_AUTO_TRUST=0` to opt out; a visible trust prompt or missing
readiness then fails closed and preserves attach/log evidence for inspection.

For manual lane recovery, first claim the lane and keep the printed `run=`.
Then `session start WORKFLOW ISSUE --lane main|review|merge --run RUN --write`
starts the configured local tmux command with the lane-specific prompt only
after confirming that the Project claim field already matches the issue, lane,
and run. Manual claim evidence is truthful non-tmux registry evidence; `session
start` is the step that creates attach/log evidence for a real tmux session and
never writes claim fields. Main and Merge default to `tmux.agent_command`;
Review uses `tmux.review_agent_command` when set and otherwise uses
`review_lane.gemini_command` for `review_lane.backend: gemini-cli`. The rendered prompt
includes the assigned `run=` and registry pointer so the spawned agent can
preserve that value in its handoff evidence.
`session list WORKFLOW` shows active tmux sessions with attach commands, and
`session attach WORKFLOW SESSION` prints the exact attach command without
joining the terminal unless `--exec` is provided.
`status` and `status serve` include registered tmux session summaries from the
durable session registry. `doctor` flags stale, failed, orphaned, usage-limited,
or runtime/session mismatch cases, while `clean audit` classifies the registry,
rendered prompts, tmux logs, and individual sessions without deleting them.
Unknown persisted registry status values are tolerated on read: they classify
as `unknown`, keep the raw status value in diagnostics, and are not migrated,
repaired, or rewritten by normal read-only commands.
After a successful Main handoff to `Agent Review`, Jade Symphony preserves the
tmux log and attach command but reconciles matching Main session records to
`completed` and clears matching active runtime state. This keeps `doctor` from
treating a completed handoff pane as active work while preserving recovery
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
cargo run -- workspace list workflows/jade-symphony.md
cargo run -- workspace show workflows/jade-symphony.md '#253'
cargo run -- workspace adopt workflows/jade-symphony.md '#253' /tmp/jade-symphony-issue-253 --write
cargo run -- workspace ensure workflows/jade-symphony.md '#253' --dry-run
cargo run -- workspace ensure workflows/jade-symphony.md '#253' --pr 254 --write
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
branch topology from GitHub native parent/subissue links plus Jade-owned branch
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
| `project workpad` | Upsert the canonical Main Agent Workpad marker comment. | Use for Main implementation evidence, including Main-lane `Rework` implementation rounds. Repeated canonical workpad writes replace the prior canonical workpad entry instead of creating multiple top-level `Jade Symphony Workpad` blocks. Append-only `Jade Symphony Rework Run` comments explain why Rework was triggered; Review, Merge, Human Review, and Doctor evidence should remain append-only timeline comments created by their lane commands. |
| `project timeline-comment` | Append one standalone issue timeline comment from a Markdown file. | Use for lane evidence that must not overwrite the Main Agent Workpad, especially Human Review decision notes or operator-authored Doctor/repair evidence. |
| `project link-pr` | Repair PR linkage when Project readback cannot already see the PR. | First checks linked-PR readback and skips the fallback comment when linkage is already visible; if GitHub Project v2 still cannot expose the PR, it may post a linkage repair comment as a fallback. |
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
large comment.

Examples:

```bash
cargo run -- project set-state workflows/jade-symphony.md '#123' need_to_clarify --write
cargo run -- project workpad workflows/jade-symphony.md '#123' /tmp/workpad.md --write
cargo run -- project timeline-comment workflows/jade-symphony.md '#123' /tmp/human-review-note.md --write
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
cargo run -- clean plan workflows/jade-symphony.md
cargo run -- clean audit workflows/jade-symphony.md
```

## Issue Readiness And Contract Validation

| Command | Purpose | Boundary |
| --- | --- | --- |
| `forge validate` | Validate issue contract quality for a draft body or existing issue. | Read-only; `Todo` uses the full Issue Quality Gate, `Backlog` uses the lighter seed gate. |
| `project inspect` | Read live Project readiness, blockers, linked PR evidence, and dispatchability for one issue. | Read-only; does not claim, route, write workpads, or change status. |

Examples:

```bash
cargo run -- forge validate --workflow workflows/jade-symphony.md --issue '#123'
cargo run -- project inspect workflows/jade-symphony.md '#123'
```

## Issue Forge

| Command | Purpose | Boundary |
| --- | --- | --- |
| `forge validate` | Validate a body file, an existing issue, or candidate title/body content against live issue context for `Backlog` or `Todo`. | Read-only; `Todo` uses the full Issue Quality Gate, `Backlog` uses the lighter seed gate; output separates candidate contract gaps from live context gaps. |
| `forge create` | Create a Project-backed issue in `Backlog` or `Todo`. | Dry-run by default unless `--write` is supplied; initializes the Project item to the requested status and verifies readback; write success keeps `issue_id` and adds readback issue number, URL, and `project_status` when the tracker provides them; live `Todo` creation requires `--assignee`. |
| `forge promote` | Promote one existing Backlog issue in place by editing title/body, writing a structured Promotion Note comment, then moving it to `Todo`. | Dry-run by default unless `--write` is supplied; requires structured note inputs, keeps the `Todo` status mutation last, and reports the checkpoint where any failure stopped. |
| `forge rework` | Revise one live `Human Review` issue into an explicit `Rework` contract. | Dry-run by default unless `--write` is supplied; requires a replacement title/body, evidence file, and operator confirmation; rejects active lane claims and keeps the `Rework` status mutation last. |

Examples:

```bash
cargo run -- forge validate --workflow workflows/jade-symphony.md --status Backlog --title "Backlog seed" --body-file /tmp/issue.md
cargo run -- forge validate --workflow workflows/jade-symphony.md --status Todo --title "Executable issue" --body-file /tmp/issue.md
cargo run -- forge validate --workflow workflows/jade-symphony.md --issue '#293' --status Todo --title "Candidate promoted title" --body-file /tmp/candidate.md
cargo run -- forge create --workflow workflows/jade-symphony.md --status Backlog --title "Backlog: follow-up title" --body-file /tmp/issue.md --dry-run
cargo run -- forge create --workflow workflows/jade-symphony.md --status Todo --title "Follow-up title" --body-file /tmp/issue.md --assignee Alive24 --write
cargo run -- forge promote '#241' --workflow workflows/jade-symphony.md --title "Executable title" --body-file /tmp/issue.md --operator-confirmation "promote it" --decision "Use the CLI-owned promotion note template." --scope-change "Backlog seed is now an executable Todo issue." --dependency-context "Dependencies: none; related context is non-blocking." --readback-summary "Operator confirmed the dry-run preview before write." --dry-run
cargo run -- forge promote '#241' --workflow examples/promote-fixture-workflow.md --title "Harden Issue Forge Reflect promotion fixture" --body-file examples/fixtures/promoted-issue.md --operator-confirmation "promote it" --decision "Keep the promotion in place." --scope-change "Backlog seed becomes an executable Todo issue." --dependency-context "Dependencies: none." --readback-summary "Dry-run preview verified before write." --dry-run
cargo run -- forge rework '#282' --workflow workflows/jade-symphony.md --title "Rework: revised execution contract" --body-file /tmp/rework-body.md --evidence-file /tmp/rework-evidence.md --operator-confirmation "route Human Review back to Rework" --dry-run
```

Successful `forge create --write` output is a single parseable line. It keeps
the machine-facing tracker node id while exposing live tracker readback metadata
for operator lookup:

```text
forge_create=ok issue_id=I_kw... issue=#305 url=https://github.com/Alive24/jade-symphony/issues/305 status=Backlog project_status=Backlog project_fields=0
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
| `review loop` | Bounded review worker selection/reconciliation. | For `gemini-cli`, runs headless Gemini by default with stdin prompt transport, JSON output capture, configured model/tools, durable review-job evidence, and health-aware retry routing. |
| `review status` | Read review-loop and review-runner status from local ledgers, runtime/session registry, and Project claim cross-checks. | Read-only; never claims, repairs, retries, kills jobs, writes workpads, or changes Project state. |
| `review claim` | Claim one `Agent Review` item's `Review Agent` text field for manual/operator review. | Requires `--worker` and `--write`; refuses non-`Agent Review` issues and writes a structured, round-trip-validated claim pointer. |
| `review pass` | Record manual independent review pass evidence and route to the correct next state. | Requires `--write`, a durable evidence file containing the exact current `Review Agent` claim, and preserves the field as terminal pass evidence. Ordinary issues and parent final issues route to `Human Review`; routine native subissues route directly to `Merging` unless they record `Subissue Human Review Exception: <reason>`. |
| `review reject` | Record failed/inconclusive manual review evidence and route to `Agent Review`, `Rework`, or `Need Human Input`. | Refuses `Human Review`, requires exact claim evidence, and preserves the field as terminal reject/failed evidence. |
| `review session` | Hidden legacy review session alias. | Does not write the `Review Agent` claim; use `review claim` or `review loop` for claim ownership. |
| `review freshness` | Record/inspect review freshness evidence. | Used around merging/rework conflict repair. |
| `review-clear-claim` | Clear one issue's `Review Agent` claim through the tracker adapter. | Requires `--write`; use after terminal manual review routing. |
| `session start` | Start an attachable local tmux session for a selected lane and `run`. | Manual recovery path; validates an existing lane claim, selects the lane-specific command, and does not write Project claim fields. |
| `session list` | List active Jade Symphony tmux sessions by configured prefix. | Read-only operator summary. |
| `session attach` | Print or execute the tmux attach command for one session. | Defaults to printing the command; `--exec` enters tmux. |

Example:

```bash
cargo run -- review loop examples/review-fixture-workflow.md --max-iterations 1 --dry-run
cargo run -- review loop workflows/jade-symphony.md --max-iterations 1 --write
cargo run -- review status workflows/jade-symphony.md
cargo run -- review status workflows/jade-symphony.md --issue '#226' --recent 3 --verbose
cargo run -- review status workflows/jade-symphony.md --json
cargo run -- review claim workflows/jade-symphony.md '#226' --worker "Manual Gemini Review" --write
cargo run -- session start workflows/jade-symphony.md '#226' --lane review --run <RUN_ID> --write
cargo run -- session list workflows/jade-symphony.md
cargo run -- review pass workflows/jade-symphony.md '#226' --evidence-file /tmp/review-evidence.md --write
cargo run -- review reject workflows/jade-symphony.md '#226' --evidence-file /tmp/review-evidence.md --target-state rework --write
```

Manual review evidence files must include the exact structured `Review Agent`
claim printed by `review claim` or recorded by `review loop`. Terminal routing
updates that same field to an audit pointer such as `state=done result=passed`,
`state=done result=rejected`, `state=failed result=inconclusive`, or
`state=failed result=blocked`; it does not clear the field.

Gemini-backed `review loop` distinguishes recoverable backend health from
operator-action blockers. Quota, rate-limit, and resource-exhausted responses
wait and retry when the loop is allowed to continue; transient capacity,
network, timeout, or 5xx failures retry with bounded backoff while keeping the
issue in `Agent Review`. Command, auth, model, policy, or allowed-tools
configuration failures route to `Need Human Input`. Repeated same-cause Gemini
failures append compact repeat evidence instead of duplicating full logs.

## Local Skill Suite

Repo-packaged Jade Symphony skills live under `skills/jade-symphony/` with
release metadata in `skills/jade-symphony/manifest.toml`. The installer previews
and updates local Codex and Gemini skill directories:

```bash
node scripts/install-jade-symphony-skills.js --dry-run
node scripts/install-jade-symphony-skills.js
node scripts/install-jade-symphony-skills.js --validate
node scripts/install-jade-symphony-skills.js --codex-dir "$HOME/.codex/skills" --gemini-dir "$HOME/.gemini/local-skills" --yes
```

Normal install mode is interactive: it prints detected target paths and requires
operator confirmation before writing. Use `--skip-codex`, `--skip-gemini`,
`--codex-dir`, and `--gemini-dir` for manual target control. Validation compares
the active local skill files with the repo-owned dated suite.
`doctor` also reports read-only install-health warnings for the detected Codex
and Gemini skill roots, including missing roots, broken links, file-shaped
aliases, missing `SKILL.md`, stale metadata, and stale Jade Symphony CLI naming.
It points back to this installer path for repair instead of mutating local
skills directly.

The suite packages Issue Forge, Issue Forge Reflect, Issue Forge Dream, Manual
Main, Manual Review, Human Review, Manual Merge, and a Doctor/Fix stub. Human
Review is an operator-owned briefing and UAT decision skill: it records a
structured decision note and routes to `Merging`, `Rework`, or
`Need Human Input` only after explicit operator confirmation. `forge reflect`
and `forge dream` remain skill behaviors, not Jade Symphony CLI subcommands.
`forge create`, `forge promote`, `forge rework`, and `forge validate` remain
deterministic CLI executor surfaces.

## Issue Forge Dream

Issue Forge Dream is a Codex/Gemini skill workflow for slow, deep backlog
mining. It reads broader Jade Symphony context, writes bounded advisory logs,
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
| `merge once` | Inspect one `Merging` issue, verify a single linked PR, and either merge, safely refresh a stale branch, attempt safe conflict repair, or route blockers. | Live merge requires explicit `--write`; fixture workflows synthesize merge or conflict-repair command evidence without touching GitHub. Native subissues expect the parent integration branch as the PR base; parent final PRs expect `main`. `BEHIND` PRs are updated with `gh pr update-branch` and left in `Merging` for retry, transient `UNKNOWN` mergeability stays in `Merging`, `DIRTY` PRs first try a clean local PR-worktree repair when available, and unrepaired dirty/failing blockers route to `Need Human Input` with a concrete question instead of defaulting to `Rework`. |
| `merge loop` | Repeat guarded merge ticks for an explicit bounded iteration count. | Requires `--max-iterations` or `--once`; `--max-concurrent N` processes up to `N` merge slots while respecting `Merging Agent` claim fields; recover-first handling is enabled by default in `--write` mode and can be disabled with `--no-recover`. |

Examples:

```bash
cargo run -- merge once workflows/jade-symphony.md --dry-run
cargo run -- merge loop examples/merge-fixture-workflow.md --max-iterations 1 --write
cargo run -- merge loop examples/merge-conflict-repair-fixture-workflow.md --max-iterations 1 --write
cargo run -- merge loop workflows/jade-symphony.md --max-iterations 2 --max-concurrent 2 --write
```

`merge once` is separate from main implementation and review work. It should
only consume issues already in `Merging`. `Rework` remains a Main/Review repair
lane unless an operator explicitly chooses a historical merge-lane recovery
path.
`merge loop --write` uses recover-first handling by default for interrupted
in-process merge runs. Because merge work has no long-lived tmux session to
probe, recovery is tracker-first: it only adopts structured active merge claims
created by loop or goal sources, leaves manual claims alone, keeps the issue in
`Merging` for safe stale-base updates or merge-lane repairs, and falls back to
normal unclaimed merge selection after recoverable claims have been handled.
Use `--no-recover` only for debugging or a deliberately conservative operator
pass.

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
