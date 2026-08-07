# Operator Dogfood Launcher

Use `scripts/shea-dogfood` for supervised local dogfood runs. It is a thin
operator entrypoint around the built `shea-symphony` binary and the GitHub
Project workflow.

It is intentionally not a daemon and does not hide write mode.

## Build

```bash
cargo build
```

## Preview

```bash
scripts/shea-dogfood --dry-run
```

The launcher checks:

- workflow file exists;
- `target/debug/shea-symphony` exists and is executable;
- current directory is inside a git repository;
- `gh` exists;
- `gh auth status` succeeds;
- the workflow validates;
- `project state` and `doctor` read the live workflow state;
- in write mode, `autopilot plan` and the bounded `autopilot loop --dry-run`
  preflight pass.

The canonical supervised operator workflow is `workflows/shea-symphony.md`. It
defaults durable worktrees, logs, and runtime artifacts under
`~/.shea-symphony/artifacts`; set
`SHEA_SYMPHONY_ARTIFACT_ROOT` before running commands to move the whole local
artifact tree.

The workflow file is an index/config, not a single prompt for every role. It
references lane prompt contracts under `workflows/prompts/`:

- `main-agent.md` for implementation ticks that stop at `Agent Review`;
- `review-agent.md` for independent review and review evidence;
- `merge-agent.md` for guarded `Merging` land/rework decisions.

Fixture workflows can still use inline prompt bodies. If the canonical workflow
declares lane prompts, all three lane paths must exist before agent
initialization continues.

Main and semantic Merge-agent repair may instead select the shared Claude Code
stream-json transport with `main_lane.backend: claude-code` and/or
`merge_lane.agent_backend: claude-code`. Shea owns the protocol flags, JSONL
input, parsing, artifacts, timeout cleanup, and safe resume; the configured
`claude.command` or wrapper owns model, gateway, authentication, environment,
and permission policy. See `docs/claude-code-stream-json.md` for configuration,
failure semantics, and the no-remote-write local UAT.

After preflight, dry-run mode executes the all-lane foreground preview:

```bash
target/debug/shea-symphony autopilot loop workflows/shea-symphony.md --max-iterations 1 --dry-run
```

Use the unified read-only preflight before any write-mode dogfood:

```bash
target/debug/shea-symphony autopilot plan workflows/shea-symphony.md
target/debug/shea-symphony autopilot plan workflows/shea-symphony.md --json
```

`autopilot plan` is not write-mode automation. It reuses the current Main,
Review, and Merge lane decision helpers to show what each lane would do, which
operator queues are parked in `Human Review` or `Need Human Input`, and whether
Doctor, canonical checkout, runtime/session evidence, or ambiguous Project
state would block the foreground all-lane write command. If it reports
`idle_but_healthy`, the lanes have no work but the system is not failing; if it
reports a blocked readiness state, resolve that evidence before running write
commands.

The normal operator path is:

1. Run `autopilot plan` and resolve readiness blockers.
2. Run a bounded foreground `autopilot loop --write` from clean canonical
   `main`.
3. Drop to `main loop`, `review loop`, or `merge loop` only for focused
   debugging, break-glass recovery, or intentionally bounded lane-specific work.

`autopilot loop` is a CLI foreground supervisor. It is not a daemon, background
service, or app-server, and it currently requires `--max-iterations N` or
`--once`. The compatibility flag still says iterations, but Autoloop run
events now treat completed lane work units as the primary throughput counter.
Supervisor cycles remain visible as lifecycle evidence through
`supervisor_cycle`; lane events and result events carry `completed_work_units`
and per-lane counters so operators can see which lane actually completed work.

For a more scannable operator view, keep the same dry-run boundary and opt into
the terminal panel:

```bash
target/debug/shea-symphony autopilot loop workflows/shea-symphony.md --max-iterations 1 --dry-run --display tui
target/debug/shea-symphony main loop workflows/shea-symphony.md --max-iterations 1 --dry-run --display tui
```

The Autoloop TUI is still a foreground command, not a daemon. It renders Main,
Review, and Merge lane cards, parked operator queues, retry/backoff rows, and a
short event log from a shared dashboard snapshot. Keep future Web UI work on
that snapshot/model boundary rather than parsing terminal-rendered strings.

Panel output keeps plain text and JSON/log evidence available by default, and
only changes output when `--display tui` is passed. The same opt-in display flag
is available on focused `main loop`, `project state`, and `doctor`.

The first slice follows the current OpenAI Codex CLI terminal direction checked
against `openai/codex` on 2026-05-15: the Codex TUI crate depends on `ratatui`
and `crossterm`, with workspace versions `ratatui 0.29.0` and `crossterm
0.28.1`. Shea Symphony uses that stack for the presentation foundation while
deliberately avoiding full-screen interaction in this issue.

## Supervised Write Tick

```bash
scripts/shea-dogfood --write --confirm-write --max-iterations 1
```

Write mode is intentionally bounded. It runs `autopilot loop` only after the
explicit confirmation flag is present. Before that mutating foreground run, the
launcher runs:

```bash
target/debug/shea-symphony project state workflows/shea-symphony.md
target/debug/shea-symphony autopilot plan workflows/shea-symphony.md
target/debug/shea-symphony doctor workflows/shea-symphony.md
target/debug/shea-symphony autopilot loop workflows/shea-symphony.md --max-iterations 1 --dry-run
```

If the normal preflight surfaces fail, the launcher exits before claiming
tracker work.
`autopilot loop --write` composes bounded Main, Review, and Merge lane work in
that order, with the run limit counted against completed lane work units rather
than a shared supervisor iteration. Each lane keeps its own status authority:
Main stops at
`Agent Review`, Review records independent evidence before Human Review routing,
and Merge consumes only `Merging` work. The loop reports lane outcomes and
parked operator queues, then returns control to the operator; it does not detach
as a service.

Treat the foreground loop as one operator-controlled supervisor over independent
lane work units. `--max-iterations` limits how long the supervisor polls and
runs lane ticks; it is not a shared cap on total completed Main, Review, and
Merge items. `--main-max-concurrent`, `--review-max-concurrent`, and
`--merge-max-concurrent` are lane-local worker limits. During a healthy
iteration, Main can make progress while Review is idle, Review can make
progress while Merge is blocked, and Merge can keep landing ready work while
Main is waiting on a slow app-server turn. Blocked, idle, busy, and slow lanes
must stay visible in the CLI/TUI/log evidence, but they should not hide ready
work in another lane unless the status names a global readiness blocker such as
Doctor, canonical checkout safety, or unrecoverable runtime ambiguity.

The canonical workflow now uses the Codex app-server Main backend with
`codex.approval_policy: never`, matching the local app-server schema. A write
tick starts one app-server turn in the prepared issue worktree, records
prompt/protocol/stderr/normalized-event artifacts, persists a session registry
record under the configured artifact root, and reconciles through verification,
PR publication, linked-PR readback, PR readiness, Main Workpad evidence, and
final `Agent Review` handoff only after the turn completes successfully.
Non-terminal, failed, usage-limited, unknown, stale, or missing-registry runtime
evidence is treated conservatively and does not launch duplicate Main Agents or
hand off incomplete work. `main_lane.backend: tmux` remains available as an
explicit fallback/debug setting. In that mode, Codex-backed tmux sessions still
capture the pane before prompt injection and can auto-advance the Codex
workspace trust prompt inside a Shea Symphony-created issue worktree. Set
`SHEA_SYMPHONY_TMUX_AUTO_TRUST=0` to opt out; when disabled, or when readiness
cannot be confirmed, the tick fails closed with attach/log evidence and does
not hand off to `Agent Review`.
Main handoff also requires the PR relationship to be visible through Shea
Symphony's Project/issue linked-PR read surface, and the linked PR must be
ready, not draft. Workpad or comment URLs can identify the intended PR, but
they are not a permanent substitute for the verified relationship. When all
other handoff evidence is valid, `main loop --write` may run `gh pr ready`
before moving the issue to `Agent Review`; if relationship verification or
readiness mutation fails, keep the issue out of `Agent Review`, route to
`Need Human Input`, and preserve the blocker in the workpad.
When Main handoff reaches `Agent Review`, Shea Symphony keeps backend artifacts
as audit evidence while marking matching Main session registry entries completed
and clearing matching active runtime state. A still-open tmux pane is not by
itself active work after that reconciliation; attach only when the registry or
doctor evidence says the run is still blocked or failed. Routine status output
reads the durable session registry, probes bounded tmux pane/log tails only for
tmux fallback sessions, and reports a conservative session classification such
as
`running`, `waiting_for_trust`, `waiting_for_approval`, `usage_limited`,
`failed`, `completed`, `stale`, or `unknown`. The status surface includes only
compact evidence snippets plus artifact, attach, or log locations; inspect the
recorded app-server artifacts or attach manually only when raw evidence is
needed.
Long-running live waits also print compact `progress ...` heartbeats to stderr
after the configured threshold, defaulting to 30 seconds. These lines identify
the wait reason, issue or PR when known, backend or child process, elapsed time,
and next expected action. They are liveness and diagnosis hints only; they do
not alter timeout, retry, routing, review, or merge behavior, and they are kept
out of JSON stdout. For local UAT, set
`SHEA_SYMPHONY_PROGRESS_HEARTBEAT_MS=1000` or another small value; set it to `0`
to suppress heartbeat output for that process.
Persisted session registry statuses that are not recognized by the current
binary are read as `unknown` without rewriting or dropping the record. Status
and doctor diagnostics preserve the raw drifted value so operators can inspect
the evidence without running a repair or migration first.
`doctor` reads the same registry and reports stale, orphaned, or attention
requiring sessions next to tracker/runtime findings. `clean audit` treats the
session registry, rendered prompts, app-server artifacts, and tmux fallback logs
as recovery evidence, and only classifies completed sessions as cleanup
candidates.
If an operator switches the workflow back to `main_lane.backend: dry-run`, the
mutating tick exits before runtime-state writes, worktree creation, Project
claims, or workpad mutation.

## Post-Merge App-Server Smoke Gate

Before using #359 or another broader Autoloop dogfood issue for a long-running
write-mode run, perform one bounded Main-lane app-server smoke after #367 is
`Done` and visible on canonical `main`. This is an evidence gate, not a
production-readiness claim.

Start with readback and dry-run preflight:

```bash
target/debug/shea-symphony project issue workflows/shea-symphony.md '#367' --json
target/debug/shea-symphony project issue workflows/shea-symphony.md '#388' --json
target/debug/shea-symphony debug workflows/shea-symphony.md
target/debug/shea-symphony main loop workflows/shea-symphony.md --max-iterations 1 --dry-run
```

The preflight must show that #367 is terminal, #388's structured blocker is no
longer active, the selected Main issue is expected, and the Main backend line
reports `backend=codex`, `backend_source=codex-app-server`,
`approval_policy=never`, and `app_server_live_smoke_ready=true`. If the dry-run
selects an unsafe or surprising issue, stop there and record the mismatch as
operator-blocked smoke evidence.

Only then run the bounded live tick:

```bash
target/debug/shea-symphony main loop workflows/shea-symphony.md --max-iterations 1 --write
```

A passing smoke leaves citeable evidence in the selected issue's Main Workpad,
the PR handoff/readiness evidence, runtime state or reconciled session registry,
the prompt/protocol/stderr/normalized-event app-server artifacts, `status`, and
`doctor` readback. The smoke is sufficient for #359 or the next dogfood plan to
cite "one bounded Main app-server write path completed with durable evidence";
it does not prove merge-agent repair behavior, unattended overnight resilience,
quota resilience, or full app-server production readiness. Merge-agent
app-server smoke remains deferred until a natural repair candidate exists.

Backend, auth, quota, GitHub API, or schema failures must be classified in the
workpad or timeline evidence. Treat them as blocked/retry guidance, not as a
passing app-server smoke.

## Parent #405 UAT Checklist

Parent #405 owns final Human Review and UAT for independent Autoloop lane
throughput. Run this checklist from a clean canonical `main` checkout after the
child slices are merged into the parent integration path or visible on `main`.

1. Run CLI preflight and capture the readiness lines:

```bash
target/debug/shea-symphony project state workflows/shea-symphony.md
target/debug/shea-symphony autopilot plan workflows/shea-symphony.md
target/debug/shea-symphony doctor workflows/shea-symphony.md
```

2. Run one dry-run iteration with intentionally uneven lane limits and capture
   stdout:

```bash
target/debug/shea-symphony autopilot loop workflows/shea-symphony.md --max-iterations 1 --dry-run --main-max-concurrent 2 --review-max-concurrent 1 --merge-max-concurrent 3
```

   Verify the output includes `order=main,review,merge`, separate
   `autopilot_loop_lane` rows, and lane-local `max_concurrent` values.

3. Run the same dry-run through the TUI and inspect the Tauri app or terminal
   panel view:

```bash
target/debug/shea-symphony autopilot loop workflows/shea-symphony.md --max-iterations 1 --dry-run --display tui --main-max-concurrent 2 --review-max-concurrent 1 --merge-max-concurrent 3
```

   Verify Main, Review, and Merge cards remain separate, parked queues remain
   visible, and a blocked/idle lane does not visually erase another ready lane.

4. If live tracker state has safe ready work in more than one lane, run one
   bounded write tick:

```bash
target/debug/shea-symphony autopilot loop workflows/shea-symphony.md --max-iterations 1 --write --main-max-concurrent 2 --review-max-concurrent 1 --merge-max-concurrent 3
```

   Verify the CLI logs and Tauri run-log view show one foreground supervisor
   result with independent lane work-unit outcomes. Do not leave the loop
   running as a persistent service; rerun another bounded iteration only when
   the operator intends another supervised pass.

5. Save the parent UAT evidence in #405 Human Review notes: command lines,
   readiness phase, lane rows/cards observed, any blocked lane reason, and the
   conclusion that supervisor lifetime and lane throughput limits are distinct.

## Evidence Timeline

Shea Symphony uses two issue-comment evidence surfaces:

- `Main Agent Workpad`: one persistent marker comment owned by the Main Agent.
  It is updated in place for implementation plan, work log, verification, PR,
  workspace, and handoff evidence. Main-lane `Rework` continues to update this
  same Workpad as the current-state implementation surface. New canonical Main
  Workpad writes supersede older canonical Workpad blocks so stale planned PR
  fields such as `Live PR: not-created` do not survive after live PR evidence
  exists.
- Append-only timeline comments: every Review, Rework, Merge, Human Review, and
  Doctor run writes a standalone comment with a human-readable GMT timestamp,
  run id, lane, actor, input state, target state, result, PR when relevant, and
  evidence summary. `Shea Symphony Rework Run` comments explain why the issue
  entered `Rework`; they do not replace the Main Agent Workpad for
  implementation evidence.

PR linkage repair should be quiet when GitHub-native linked PR readback already
shows the PR. A visible linkage repair comment or Workpad PR URL is fallback
diagnostic evidence only; it does not satisfy Main handoff linkage success when
GitHub itself still has no first-class linked PR.

Review, Merge, Human Review, and Doctor flows must not overwrite or restructure
the Main Agent Workpad. Rework-trigger diagnostics should reference Main
evidence, then write their own `Shea Symphony Agent Review Run`,
`Shea Symphony Rework Run`,
`Shea Symphony Merge Run`, `Shea Symphony Human Review Decision`, or
`Shea Symphony Doctor Triage` timeline comment. Historical issues may still
contain older mixed Workpad evidence; do not migrate or delete it during normal
dogfood.

## Review Backend Setup

For live Agent Review, make the selected backend command visible to the worker
process. `review loop` owns the Review Agent claim and final routing; the
backend process is report-only. The canonical workflow continues to use `agy`
headlessly, while `codex-app-server` and `claude-code` are available as
independent structured, read-only reviewers.

Prefer an absolute `agy` path for automatic review workers:

```bash
command -v agy
```

Then configure the workflow or operator environment with that path before
running review automation:

```yaml
review_lane:
  backend: agy-cli
  agy_command: /Users/chuntengxiao/.local/bin/agy
  agy_model: gemini-3.1-pro-preview
```

For Codex Review, the Review-specific command overrides `codex.command`; omit
it to use the shared command. The safe capability defaults are also shown
explicitly here:

```yaml
codex:
  command: codex app-server -c 'service_tier="fast"'
review_lane:
  backend: codex-app-server
  codex_approval_policy: never
  codex_thread_sandbox: read-only
  # codex_turn_sandbox_policy:
  #   type: readOnly
```

Each new Codex Review job starts a fresh thread. Only one interrupted attempt
of that same job may resume its recorded thread. Inspect the review output
artifact and ledger for the backend/thread identity plus raw protocol, stderr,
normalized-event, workspace-integrity, and routing evidence.

Claude Review follows the same independent-job boundary through the shared
Claude stream-json transport. The Review-specific command overrides
`claude.command`; omit it to use the shared command. The command or wrapper owns
Claude's read-only permission arguments:

```yaml
claude:
  command: claude
review_lane:
  backend: claude-code
  claude_command: claude --permission-mode plan
  timeout_ms: 1200000
```

Every new Claude Review job starts fresh. Only its own initialized session may
resume one interrupted attempt. Shea rejects workspace mutation and missing,
malformed, truncated, schema-incomplete, timed-out, cancelled, or ambiguous
terminal output. Artifacts and the existing Review ledger preserve the command
preview, session ID, protocol/stderr/event paths, attempt count, structured
report, workspace-integrity result, and routing outcome.

Before live dogfood, run the bounded local read-only UAT fixtures:

```bash
cargo test review::codex::tests::pass_and_confirmed_finding_preserve_structured_evidence_and_workspace --lib
cargo test review::codex::tests::new_jobs_are_fresh_and_parallel_artifacts_and_threads_are_isolated --lib
cargo test review::claude::tests::pass_and_confirmed_finding_preserve_structured_evidence_and_workspace --lib
cargo test review::claude::tests::new_jobs_are_fresh_and_parallel_artifacts_and_sessions_are_isolated --lib
```

The first fixture exercises both a clean pass and a confirmed finding with
severity, location, and evidence. The second exercises parallel fresh-thread
and artifact isolation. Both use disposable Git repositories and assert the
reviewed workspace content remains unchanged; they perform no GitHub or
Project mutation.

```bash
target/debug/shea-symphony review loop workflows/shea-symphony.md --max-iterations 1 --write
```

During supervised review-loop dogfood, use the read-only status surface before
dropping to raw logs or process inspection:

```bash
target/debug/shea-symphony review status workflows/shea-symphony.md
target/debug/shea-symphony review status workflows/shea-symphony.md --issue '#<issue>' --recent 3 --verbose
target/debug/shea-symphony review status workflows/shea-symphony.md --json
```

Default output is a compact table of running review slots and recent terminal
jobs. It includes issue, title when available, job id, backend, pid when known,
elapsed time, artifact and ledger pointers, claim summary, last event or
decision, outcome, and the last five sanitized stderr lines in a short detail
block. `--json` prints the complete structured payload for scripts. The command
does not mutate Project state, claims, workpads, ledgers, or processes; it only
composes local review job ledgers, runtime/session registry evidence, and
Project `Review Agent` claim readbacks.

Use the anomaly block to decide the next human action. It calls out stale
Project claims without active local jobs, missing or dead pids, long-running
jobs past the configured review timeout, backend binary/auth/configuration
failures, missing artifacts, inconclusive or needs-rework outcomes, and jobs
that still appear active after the issue left `Agent Review`.

For supervised manual review terminals, first use
`review claim WORKFLOW '#issue' --worker <worker> --write` on an `Agent Review`
issue. The claim records minimum `codex-app-manual` registry evidence for the
printed `run=` without pretending a tmux pane exists. Then start the runtime
with `session start WORKFLOW '#issue' --lane review --run <RUN_ID> --write`.
Session startup validates the existing Review Agent claim and writes attach/log
evidence without moving the issue to `Human Review`; this tmux path is an
explicit manual fallback, not the automatic review-loop default. The worker
value may be a display label such as `Manual agy Review`; use the claim
command so Shea Symphony can quote, escape, and validate the stored pointer
before Project mutation.

If the review backend cannot start, the Agent Review timeline comment should name the
configured command, whether worker `PATH` could resolve it, the required
operator action, and the retry command. Do not move an issue to `Human Review`
unless the Review Agent actually records passing review evidence.
If the linked PR is still draft, do not run normal review. Record invalid
handoff evidence and send the work back to Main/operator repair; `doctor repair
<issue> --mark-pr-ready --confirm-handoff-ready --write` is the explicit repair
path when the operator has confirmed the handoff is otherwise complete.

If the review backend exits, refuses the workspace trust check, times out, or produces output
that is not yet parsed into durable pass/finding evidence, the issue must stay
out of `Human Review`. Inspect the recorded tmux attach command, prompt
artifact, session registry entry, and log path, then route with `review pass` or
`review reject` only after independent review evidence exists.

If the review backend returns successfully but says it could not inspect the PR, workspace,
diff, code changes, or required handoff evidence, treat that as an automatic
Review Agent inconclusive result, not a pass. `review loop` records the
inconclusive reason in the ledger/timeline comment and routes the issue to
`Rework` so the missing evidence can be repaired before another independent
review pass.

Manual review backend or operator-supplied review notes must be routed through
`review pass` or `review reject`, which wraps the note in a
`## Shea Symphony Agent Review Run` timeline comment. Mark the inner note as
manual evidence so operators can distinguish it from automatic `review loop`
pass evidence.

Use `workflows/shea-symphony.md` for supervised review workers. Do not keep the
active review workflow only under `/tmp` or `/private/tmp`; the CLI prints
`workflow_warning=temporary_path` for those workflow files so operators can
promote reusable config into the repo.

## Local Skill Suite

Shea Symphony's local operator skills are repo-owned under
`skills/shea-symphony/` and versioned by `skills/shea-symphony/manifest.toml`.
Use the installer to preview, install, update, or validate the Codex and Gemini
copies instead of hand-copying skill files:

```bash
node scripts/install-shea-symphony-skills.js --dry-run
node scripts/install-shea-symphony-skills.js
node scripts/install-shea-symphony-skills.js --validate
```

The install path is interactive by default. It shows the detected Codex and
Gemini target directories and requires operator confirmation before writing.
Use `--codex-dir`, `--gemini-dir`, `--skip-codex`, or `--skip-gemini` to make
the target set explicit. Use `--yes` only after the printed target paths are
known and intentional.

Skills are per-repo rendered installs, not one global generic skill set. Before
starting a lane that depends on local skills, inspect readiness without writing:

```bash
cargo run -- skills status workflows/shea-symphony.md
cargo run -- skills status workflows/shea-symphony.md --json
cargo run -- skills status workflows/shea-symphony.md --session-skills-file /path/to/session-skills.txt
```

`skills status` discovers the source suite from `--suite-path`,
`SHEA_SYMPHONY_SKILL_SUITE`, the current repo's `skills/shea-symphony/suite`,
then installed-only mode when no source suite exists. It reports expected/source
skills, Codex installs, Gemini installs when configured or discoverable,
rendered metadata freshness, broken links, alias/file-shaped installs, missing
`SKILL.md`, and optional current-session visibility. If no session skill input
is provided, session visibility is `unknown`; that is diagnostic context, not a
failure. Gemini absence is not a failure unless the operator explicitly requires
Gemini for the current environment.
For target workspaces, Codex readiness inspects the selected profile working
directory's `.codex/skills`; without a target profile working directory it falls
back to the workflow repo's `.codex/skills`. It does not use `CODEX_HOME` or a
home-directory Codex skill root.

The packaged skills preserve the same lane boundaries as the Shea Symphony CLI:
Issue Forge, Reflect, and Dream handle conversation, draft shaping, backlog
mining, and promotion discussion, including Human Review -> Rework revision
discussion; the CLI owns `forge create`, `forge promote`, `forge rework`, and
`forge validate`. Manual Main stops at `Agent Review`; Manual Review owns
evidence-backed review routing; Human Review briefs the operator for UAT and
final acceptance but waits for explicit confirmation before any state change;
Manual Merge owns approved merge-lane work. `doctor` reports read-only local
install-health warnings and points operators back to the #242 install/update
path rather than repairing skill files itself.

Repository execution readiness is a separate local contract from App/backend
profiles. Run `shea-symphony-runtime-onboarding` to discover repository-owned
requirements and propose `.shea/runtime-profile.json`; the skill must receive
operator confirmation before writing. Then run
`shea-symphony profiles /absolute/path/to/.shea/workflows/target.md` from the
repository or exact issue worktree. Main blocks a required missing, malformed,
drifted, or incompatible
profile before claim and records failure evidence only under the configured
local logs root. See [Repository Runtime Profiles](runtime-profiles.md).

## Inspect And Resume

```bash
target/debug/shea-symphony project inspect workflows/shea-symphony.md '#<issue>'
target/debug/shea-symphony project state workflows/shea-symphony.md
target/debug/shea-symphony project issue workflows/shea-symphony.md '#235' --json
target/debug/shea-symphony autopilot plan workflows/shea-symphony.md
target/debug/shea-symphony debug workflows/shea-symphony.md
target/debug/shea-symphony project state workflows/shea-symphony.md --display tui
target/debug/shea-symphony doctor workflows/shea-symphony.md --display tui
target/debug/shea-symphony autopilot loop workflows/shea-symphony.md --max-iterations 1 --write
```

Use `project state` before claiming work when multiple operators are active. A
healthy read prints `project_state_access=ok`, `trusted=true`, the issue count,
and a state summary, plus a read-only `canonical_checkout` cleanliness line for
the launch checkout. A failed read prints `project_state_access=blocked`,
`trusted=false`, and a `failure_kind` such as `auth`, `network`, `rate_limit`,
`transient_backend`, `resource_limit`, `schema`, `partial_response`, `payload`,
or `missing_capability`; treat that as a blocker, not as an empty queue. HTTP
502, 503, and 504 failures are `transient_backend`; GitHub API connection
errors such as resolver/connect failures or GraphQL `Post "...": EOF` transport
closures are `network`. Both classes retry with bounded backoff rather than
being treated as owner/configuration failures.
This is a queue scan surface: it keeps lane-safe status, claim, assignee,
priority, dependency, and parent/subissue gate data while avoiding issue bodies,
comment/workpad streams, and rich linked-PR hydration.

The canonical checkout is only the harness launch directory. Do not use it as a
Main, Review, or Merge issue worktree, and do not leave runtime state, logs,
prompts, drafts, or evidence there. `autopilot loop --write`,
`main loop --write`, `review loop --write`, and `merge loop --write` refresh and
check the launch checkout before tracker mutation. From a clean attached
`main`, Shea Symphony fetches the configured upstream and performs a
canonical-only `git merge --ff-only` when local `main` is only behind. The
terminal output reports `canonical_checkout_refresh=already_current`,
`ff_only`, `would_ff_only`, or `blocked`, then prints the canonical safety line.
Tracked dirty files, detached HEAD, non-`main`, missing upstream, unclassified
untracked files, and non-fast-forward updates block until the operator repairs
the canonical checkout. Recognized local artifacts are moved to artifact
quarantine with a warning.

Use `project issue` for per-issue Project status, Project fields, blocker
relationships, claim locks, rich issue body, workpad/timeline comments, native
topology evidence, and linked PRs. `project inspect` uses the same targeted rich
read for readiness checks. Raw `gh issue view` and `gh pr view` remain
acceptable for ordinary issue/PR body text, comments, and diff context, when the
CLI does not expose the needed content read; record that as a CLI gap when it
affects a workflow decision. Normal dogfood should not read or mutate Project
fields, status, claim locks, relationships, workpads, or linked-PR handoff state
through raw Project GraphQL or the Project UI. Those are break-glass recovery
paths. The current inventory and classification live in
`docs/github-access-policy.md`.

For parent tracking issues with native GitHub subissues, use
`docs/parent-subissue-topology.md` as the design source. Native sub-issue links
define hierarchy, subissue PRs target the parent integration branch by default,
routine subissue Review PASS routes directly to `Merging`, and the parent issue
remains the final Human Review and UAT unit. Direct subissue Human Review
requires an explicit `Subissue Human Review Exception: <reason>` in the issue
contract or Project evidence. `doctor` now reports read-only topology blockers
for native subissue PRs targeting `main`, missing or ambiguous parent
integration branch evidence, `Done` subissues without merge evidence into the
parent branch, and parent `Human Review` before all native subissues are `Done`
and merged.

Lane handoff and merge flows must make branch target evidence explicit. A
subissue keeps its normal `feature/issue-*` head branch but uses the parent
integration branch as the PR base. A parent final PR uses the parent integration
branch as its head and `main` as its base. Workpads and PR bodies should record
the native parent issue, `parent_integration_branch`, PR base branch, and parent
final base branch when applicable so Review, Doctor, and Merge read the same
topology evidence.

## Issue Forge Reflect

Issue Forge Reflect is a Codex skill workflow, not a Shea Symphony CLI
subcommand. Use it to turn recent dogfood conversations, operator notes, or
Project observations into non-dispatchable `Backlog` seeds, then use the
conversation-led promote flow when an operator selects one seed for execution.

Backlog capture should stay intentionally light:

- create only enough body context to revisit the candidate later;
- keep Project status as `Backlog`;
- do not treat the seed as executable work;
- use `forge create --status Backlog` for the actual tracker mutation.

Promotion is stricter. Before mutation, resolve scope, dependencies,
verification, UAT, and the exact promoted title/body with the operator. Require
an explicit confirmation phrase, then run `forge promote` with the structured
Promotion Note fields. In write mode, `forge promote` edits the same issue,
writes the Promotion Note, moves status from `Backlog` to `Todo` as the final
mutation, and only then performs read-only status readback. Do not start
Main/Review/Merge work in that same promotion session unless the operator
explicitly starts a new cycle.

Human Review -> Rework revision is also Issue Forge-owned. Use discussion to
prepare the full replacement Rework body, evidence file, title, and explicit
operator confirmation, then run `forge rework`. The command is non-interactive:
it validates the source issue is `Human Review`, rejects active lane claims,
records a diagnostic workpad if they are present, preserves terminal lane claims
as audit pointers, replaces the issue content, writes Rework revision evidence
as an append-only `Shea Symphony Rework Run` timeline comment, and sets
`Rework` as the final mutation. Do not use raw Project
mutation, `project set-state`, or `forge promote` for this normal path. Missing
linked PRs or missing local worktrees are downstream Main Agent recovery work
after the issue is in `Rework`.

When the operator wants to preflight a candidate body for an existing issue
without running the promotion command, use `forge validate --issue '#123'
--status Todo --title "<candidate title>" --body-file <path>`. That mode is
read-only: it validates the candidate title/body while reusing live assignee and
Project context from the issue, and reports candidate contract gaps separately
from live-context gaps.

Use `debug` when you need one read-only operator report before a supervised
dogfood, repair, review, or merge session. It summarizes the current Project
queue, doctor health, smoke readiness, runtime/session state, cleanup/audit
status, and lane-specific next commands without claiming work, starting workers,
repairing state, cleaning artifacts, or implying unattended readiness.

## Issue Forge Dream

Issue Forge Dream is the slow backlog-mining companion to Reflect. Use Dream
when the operator wants to sleep on broader Shea Symphony history: recent
Project state, run logs, workpads, Doctor findings, repo-owned skills, memory
summaries, bootstrap docs, and recent docs/code drift.

Dream creates enriched `Backlog` seeds by default unless the operator requests
report-only mode. It never creates `Todo` issues directly. Dream Logs live under
`docs/dream-log/YYYY-MM-DD-<run-count>-<slug>/`, with
`docs/dream-log/INDEX.md` as the compact global entrypoint. A normal Dream run
reads the index plus the most recent five Dream runs, writes bounded `RUN.md`
and `topic-*.md` evidence, records lightweight Gemini review status, and reports
whether it slept enough plus the next useful Dream theme.

Dream Logs are advisory. Dream, Reflect, and Issue Forge may read them actively;
Main reads only Dream Logs explicitly referenced by an issue contract; Review
reads them only when the PR or issue body involves Dream-derived context; Merge
usually ignores them; Doctor may use them only as advisory context, not
workflow invariants.

Use the repo-owned Doctor skill at
`.codex/skills/shea-symphony-doctor/SKILL.md` when an operator-selected issue or
`Need Human Input` item needs triage before normal lane work can resume. The
skill is read-first: it gathers `project state`, `doctor`, `debug`, and
`project issue` evidence, classifies the stuck state, and produces a structured
`Doctor Triage Note` with any repair actions that still require explicit
confirmation. Local skill install checking is reported by `doctor`, while dated
installable skill suite packaging and writes remain in the #242 installer path.

If `main loop` finds runtime-state for an issue that has already moved out of
active main-agent work, it reconciles tracker state first. Clean or absent
workspaces are archived under the configured runtime log directory and the loop
continues; dirty or unknown workspaces still stop the loop with a repair
diagnostic so local work is not discarded silently.

`doctor` treats Human Review as valid only when independent review pass evidence
is durable. Project fields named `review_pass_evidence_recorded` or
`review_pass_evidence` satisfy that check when a tracker exposes them; in the
current GitHub Project #9 schema, the canonical source is an Agent Review
timeline comment in the issue comment stream. A `Review Agent` claim by itself
is not pass evidence.

For a manual `agy`/operator review, claim and route through the CLI:

```bash
target/debug/shea-symphony review claim workflows/shea-symphony.md '#226' --worker "Manual agy Review" --write
target/debug/shea-symphony review pass workflows/shea-symphony.md '#226' --evidence-file /tmp/review-evidence.md --write
target/debug/shea-symphony review reject workflows/shea-symphony.md '#226' --evidence-file /tmp/review-evidence.md --target-state rework --write
```

The evidence file for `review pass` or `review reject` must include the exact
structured `Review Agent` claim from `review claim`. `review pass` writes an
append-only Agent Review timeline comment with the review pass marker before
moving ordinary and parent final issues to `Human Review`, or routine native
subissues to `Merging`; `review reject` refuses
`Human Review` and may route only to `Agent Review`, `Rework`, or
`Need Human Input`. Both commands preserve the `Review Agent` field as terminal
audit evidence instead of clearing it.

## Artifact Root Migration

To move local runtime artifacts without changing repo-owned workflow files, set
one environment variable before launching dogfood commands:

```bash
export SHEA_SYMPHONY_ARTIFACT_ROOT="$HOME/.shea-symphony/artifacts"
```

The live operator workflow derives implementation and review worktree/log paths
from that root. Existing temp Markdown files should be classified before
cleanup: normal operator workflow config belongs in `workflows/`, fixtures and
reference examples belong in `examples/`, reusable operator prompts belong in
`docs/`, issue and PR drafts belong in tracker/workpad or log artifacts, and
disposable scratch can be removed only through a separate cleanup decision.

Use the grouped `clean` surface for local cleanup and persistence questions:

```bash
target/debug/shea-symphony clean plan workflows/shea-symphony.md
target/debug/shea-symphony clean audit workflows/shea-symphony.md
```

`clean plan` is the grouped form of the existing read-only cleanup plan, while
`clean audit` classifies configured artifact/workspace residue as
`promote_to_repo`, `attach_to_tracker`, `safe_to_keep`, `cleanup_candidate`, or
`needs_human_decision`. Keep `doctor` for tracker/runtime invariants and stuck
workflow states.

Interrupted runtime recovery flow:

1. Run `target/debug/shea-symphony status workflows/shea-symphony.md` and read
   the `runtime sessions` section for backend, session status, artifact path,
   attach command when available, and log.
2. Run `target/debug/shea-symphony doctor workflows/shea-symphony.md` before
   retrying or clearing runtime state; stale, failed, usage-limited, or
   unattributed sessions require operator inspection.
3. For normal all-lane recovery, start with the same foreground Autoloop path:

```bash
target/debug/shea-symphony autopilot plan workflows/shea-symphony.md
target/debug/shea-symphony autopilot loop workflows/shea-symphony.md --max-iterations 1 --write
```

Autoloop (`autopilot loop --write`) uses recover-first handling for Main and Merge lanes by
default while still preserving Review's independent evidence boundary. It is the
normal supervised dogfood path when the operator is not deliberately isolating
one lane.

4. For focused Main-lane runtime work where the issue is still `In Progress`,
   run a bounded lane recovery tick:

```bash
target/debug/shea-symphony main loop workflows/shea-symphony.md --max-iterations 1 --max-concurrent 3 --write
```

`main loop --write` restarts recoverable Main runtime slots as new attempts by
default without moving the issue to `Rework`, clearing dirty worktrees, or
advancing to `Agent Review`. It reuses a tracker/runtime/discovery-backed git
worktree under the configured workspace root. Normal recovery leaves handoff to
a later successful Main result. If a Codex app-server attempt stalls after local
work but before a final turn event, the loop first attempts the same live
handoff pipeline that a successful Main result would use; verification, PR
publication, Project-visible PR linkage, and ready-for-review checks still gate
any `Agent Review` transition. Codex app-server session staleness defaults to
30 minutes and can be tuned with `codex.session_stale_after_ms` in the workflow.
Codex app-server turn inactivity defaults to 5 minutes and can be tuned with
`codex.stall_timeout_ms`; a turn that starts but produces no further protocol
events is terminated and retried rather than waiting for the full turn timeout.
When a registered Codex app-server session is stale and has recorded process
evidence, recovery terminates that stale process before resuming the saved
thread with a fresh `Continue` turn. A dirty worktree is acceptable only for a
recoverable `In Progress` Main runtime when the branch or path still matches the
same issue; detached, ambiguous, or mismatched dirty worktrees still require
human inspection. Use `--no-recover` only for debugging or a deliberately
conservative operator pass.
5. For focused Merge-lane loop work where the issue is still `Merging`, run
   a bounded recovery tick:

```bash
target/debug/shea-symphony merge loop workflows/shea-symphony.md --max-iterations 1 --max-concurrent 2 --write
```

`merge loop --write` adopts interrupted structured merge-loop/goal claims first
by default, then continues normal merge selection. It leaves manual claims
alone, keeps safe stale-base refreshes or merge-lane repairs in `Merging`, and
routes serious blockers to `Need Human Input` rather than `Rework`. Use
`--no-recover` only for debugging or a deliberately conservative operator pass.

6. Run `target/debug/shea-symphony clean audit workflows/shea-symphony.md` only
   after evidence is preserved. Active or uncertain sessions stay
   `needs_human_decision`; completed sessions and terminal clean worktrees may
   become cleanup candidates.

For supervised parallel operators, pass `--max-concurrent N` to preview eligible
slots and apply lane-specific claim checks. Main work uses the `Main Agent`
Project field as the claim lock and the runtime-state file as the local worker
slot ledger. In write mode, `main loop` first counts active Main runtime entries
that still point at `In Progress` issues, archives clean stale handoff entries,
and then starts up to the remaining capacity in the same bounded iteration.
Existing single-entry runtime-state files still load, but once multiple Main
workers are active the file stores an `active_workers` list so one issue cannot
overwrite another issue's tmux/session evidence. Merge work uses the `Merging
Agent` Project field and can process multiple guarded merge slots in one
bounded loop.
Lane claim fields are latest-run audit pointers, not append-only logs. New
values use `v=1 lane=<main|review|merge> actor=<codex|gemini|antigravity|claude|human>
worker=<worker> source=<loop|manual|goal> issue=#N run=<id>
state=<active|done|stale|failed|superseded> thread=<codex-link|unknown>
registry=run/<id>`. Keep full paths and terminal logs in the session registry
or workpad, and update terminal completed work to `state=done` instead of
clearing useful claim evidence by default. Terminal handoffs also append a
compact `result=...` token so operators can distinguish `agent_review_handoff`,
Review `passed` or `rejected`, and Merge `merged` audit pointers from live
ownership. Display labels with spaces are stored with reversible quoting, for
example `worker="Codex Manual Main"`; raw Project field edits are a break-glass
repair path, not normal claim ownership.
If GitHub returns a transient transport, rate-limit, or HTTP 5xx error after a
claim, workpad, timeline comment, Project status, merge, or issue-close write,
the CLI performs read-after-write reconciliation instead of blindly retrying the
mutation. A recovered write prints `tracker_recovery action=recovered` and the
lane continues. An uncertain write prints `recoverable_tracker_mutation_uncertain`
with the mutation type, issue or PR, failure kind, and next safe action; rerun
the same lane command after waiting or inspect with `project issue`. Do not
clear lane claims to recover uncertainty, and do not send merge transport
failures to `Rework`. Already-applied recovery checks are intentionally quiet in
default output; use issue readback, event logs, or verbose diagnostics when you
need to inspect idempotent reruns.
For supervised merge terminals, use `merge claim WORKFLOW '#issue' --worker
<worker> --write` on a `Merging` issue; the claim records truthful non-tmux
manual evidence for the `run=`. Then use `session start WORKFLOW '#issue'
--lane merge --run <RUN_ID> --write` when a supervised tmux terminal is needed.
Session startup validates the `Merging Agent` field and writes attach/log
evidence without merging the PR or closing the issue.

Operator commands print compact issue-scoped `Latest:` lines for the current
lane, issue, category, action, actor, workspace/branch when known, and next
expected step. Treat these as the glanceable status bar; no-issue idle ticks and
runtime telemetry belong to JSON/status details, while JSONL events remain the
durable audit trail. Autoloop invokes lane ticks in quiet mode by default, so
iteration counters, selected-none stops, already-queued skips, and bare
`reason=` / `pull_request=` diagnostics do not enter the operator stream unless
you opt into verbose diagnostics.

```bash
target/debug/shea-symphony main loop workflows/shea-symphony.md --max-iterations 1 --max-concurrent 2 --dry-run
target/debug/shea-symphony autopilot loop workflows/shea-symphony.md --max-iterations 1 --dry-run
target/debug/shea-symphony merge loop workflows/shea-symphony.md --max-iterations 1 --max-concurrent 2 --dry-run
```

## Logical Actor Audit

Dogfood can run many local workers through the same GitHub account. GitHub will
show the configured account for API mutations, so Shea Symphony also writes a
local `tracker_mutation` audit record to the configured event log. The record
captures the logical actor role and label, git author when configured, command,
mutation type, issue, target, from/to state when known, reason, and timestamp.

Use this audit trail to distinguish `main_agent`, `review_agent`,
`merge_agent`, operator repair, and Issue Forge activity without requiring
multiple GitHub users or tokens. Audit records must not contain secrets; token
or authorization-shaped text is redacted before serialization.

## Cleanup Planning

Cleanup planning is read-only:

```bash
target/debug/shea-symphony clean plan workflows/shea-symphony.md
target/debug/shea-symphony clean audit workflows/shea-symphony.md
target/debug/shea-symphony clean plan workflows/shea-symphony.md
```

`clean plan` reports terminal worktrees that appear removable only when tracker
state is terminal, the linked PR is merged or closed, the local worktree branch
matches the issue branch, and the worktree is clean. `clean plan` remains a
compatibility path for the same read-only behavior.

`clean audit` classifies local artifact and workspace residue by persistence
need. It never deletes files.

Do not use this launcher to bypass Agent Review, Human Review, or Merging role
boundaries.
