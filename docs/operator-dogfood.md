# Operator Dogfood Launcher

Use `scripts/jade-dogfood` for supervised local dogfood runs. It is a thin
operator entrypoint around the built `jade-symphony` binary and the GitHub
Project workflow.

It is intentionally not a daemon and does not hide write mode.

## Build

```bash
cargo build
```

## Preview

```bash
scripts/jade-dogfood --dry-run
```

The launcher checks:

- workflow file exists;
- `target/debug/jade-symphony` exists and is executable;
- current directory is inside a git repository;
- `gh` exists;
- `gh auth status` succeeds;
- the workflow validates;
- `project state` and `doctor` read the live workflow state;
- in write mode, the bounded `main loop --dry-run` preflight passes.

The canonical supervised operator workflow is `workflows/jade-symphony.md`. It
defaults durable worktrees, logs, and runtime artifacts under
`~/.jade-symphony/artifacts`; set
`JADE_SYMPHONY_ARTIFACT_ROOT` before running commands to move the whole local
artifact tree.

The workflow file is an index/config, not a single prompt for every role. It
references lane prompt contracts under `workflows/prompts/`:

- `main-agent.md` for implementation ticks that stop at `Agent Review`;
- `review-agent.md` for independent review and review evidence;
- `merge-agent.md` for guarded `Merging` land/rework decisions.

Fixture workflows can still use inline prompt bodies. If the canonical workflow
declares lane prompts, all three lane paths must exist before agent
initialization continues.

After preflight, dry-run mode executes:

```bash
target/debug/jade-symphony main loop workflows/jade-symphony.md --max-iterations 1 --dry-run
```

For a more scannable operator view, keep the same dry-run boundary and opt into
the terminal panel:

```bash
target/debug/jade-symphony main loop workflows/jade-symphony.md --max-iterations 1 --dry-run --display tui
```

The panel view is not a full-screen dashboard. It keeps plain text and JSON/log
evidence available by default, and only changes output when `--display tui` is
passed. The same opt-in display flag is available on `project state` and
`doctor`.

The first slice follows the current OpenAI Codex CLI terminal direction checked
against `openai/codex` on 2026-05-15: the Codex TUI crate depends on `ratatui`
and `crossterm`, with workspace versions `ratatui 0.29.0` and `crossterm
0.28.1`. Jade Symphony uses that stack for the presentation foundation while
deliberately avoiding full-screen interaction in this issue.

## Supervised Write Tick

```bash
scripts/jade-dogfood --write --confirm-write --max-iterations 1
```

Write mode is intentionally bounded. It runs one `main loop` tick only after the
explicit confirmation flag is present. Before that mutating tick, the launcher
runs:

```bash
target/debug/jade-symphony project state workflows/jade-symphony.md
target/debug/jade-symphony doctor workflows/jade-symphony.md
target/debug/jade-symphony main loop workflows/jade-symphony.md --max-iterations 1 --dry-run
```

If the normal preflight surfaces fail, the launcher exits before claiming
tracker work.
The canonical workflow now uses the local `tmux` main-agent backend. A write
tick starts an attachable tmux session, records its attach command and log path,
persists a session registry record under the configured artifact root, and
leaves the issue active until real implementation/handoff evidence exists.
When the recorded Main session later reaches a terminal completed state,
another bounded `main loop --write` tick reconciles it through verification, PR
publication, linked-PR readback, PR readiness, Main Workpad evidence, and final
`Agent Review` handoff. Non-terminal, waiting, unknown, or missing-registry
session evidence is treated as incomplete work and does not launch a duplicate
Main Agent.
For Codex-backed tmux sessions, Jade Symphony captures the pane before prompt injection.
If the Codex workspace trust prompt is visible in a Jade Symphony-created issue worktree,
the backend sends two `C-m` submissions, waits for a ready Codex viewport, and
only then pastes the rendered issue prompt. Set
`JADE_SYMPHONY_TMUX_AUTO_TRUST=0` to opt out; when disabled, or when readiness
cannot be confirmed, the tick fails closed with attach/log evidence and does not
hand off to `Agent Review`.
Main handoff also requires the PR relationship to be visible through Jade
Symphony's Project/issue linked-PR read surface, and the linked PR must be
ready, not draft. Workpad or comment URLs can identify the intended PR, but
they are not a permanent substitute for the verified relationship. When all
other handoff evidence is valid, `main loop --write` may run `gh pr ready`
before moving the issue to `Agent Review`; if relationship verification or
readiness mutation fails, keep the issue out of `Agent Review`, route to
`Need Human Input`, and preserve the blocker in the workpad.
When Main handoff reaches `Agent Review`, Jade Symphony keeps tmux logs and
attach commands as audit evidence while marking matching Main session registry
entries completed and clearing matching active runtime state. A still-open tmux
pane is not by itself active work after that reconciliation; attach only when
the registry or doctor evidence says the run is still blocked or failed.
Routine status output reads the durable session registry, probes bounded pane
and log tails, and reports a conservative session classification such as
`running`, `waiting_for_trust`, `waiting_for_approval`, `usage_limited`,
`failed`, `completed`, `stale`, or `unknown`. The status surface includes only
compact evidence snippets plus attach/log locations; attach manually when raw
scrollback is needed.
Persisted session registry statuses that are not recognized by the current
binary are read as `unknown` without rewriting or dropping the record. Status
and doctor diagnostics preserve the raw drifted value so operators can inspect
the evidence without running a repair or migration first.
`doctor` reads the same registry and reports stale, orphaned, or attention
requiring sessions next to tracker/runtime findings. `clean audit` treats the
session registry, rendered prompts, and tmux logs as recovery evidence, and only
classifies completed sessions as cleanup candidates.
If an operator switches the workflow back to `main_lane.backend: dry-run`, the
mutating tick exits before runtime-state writes, worktree creation, Project
claims, or workpad mutation.

## Evidence Timeline

Jade Symphony uses two issue-comment evidence surfaces:

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
  evidence summary. `Jade Symphony Rework Run` comments explain why the issue
  entered `Rework`; they do not replace the Main Agent Workpad for
  implementation evidence.

PR linkage repair should be quiet when normal GitHub/Project readback already
shows the PR. A visible linkage repair comment is a fallback for cases where
GitHub Project v2 cannot otherwise expose the PR, not routine timeline noise.

Review, Merge, Human Review, and Doctor flows must not overwrite or restructure
the Main Agent Workpad. Rework-trigger diagnostics should reference Main
evidence, then write their own `Jade Symphony Agent Review Run`,
`Jade Symphony Rework Run`,
`Jade Symphony Merge Run`, `Jade Symphony Human Review Decision`, or
`Jade Symphony Doctor Triage` timeline comment. Historical issues may still
contain older mixed Workpad evidence; do not migrate or delete it during normal
dogfood.

## Review Backend Setup

For live Agent Review, make the Gemini command visible to the worker process.
`review loop` claims the Review Agent field and runs Gemini headlessly by
default with `--prompt`, `--output-format json`, the configured model, and the
configured interim allowed tools. Prompt content is written through stdin so
long prompts are not passed through argv or TUI paste buffers.

Prefer an absolute Gemini path for automatic review workers:

```bash
command -v gemini
```

Then configure the workflow or operator environment with that path before
running review automation:

```yaml
review_lane:
  backend: gemini-cli
  gemini_command: /opt/homebrew/bin/gemini
  gemini_model: gemini-3.1-pro-preview
  gemini_allowed_tools:
    - run_shell_command
```

```bash
target/debug/jade-symphony review loop workflows/jade-symphony.md --max-iterations 1 --write
```

During supervised review-loop dogfood, use the read-only status surface before
dropping to raw logs or process inspection:

```bash
target/debug/jade-symphony review status workflows/jade-symphony.md
target/debug/jade-symphony review status workflows/jade-symphony.md --issue '#<issue>' --recent 3 --verbose
target/debug/jade-symphony review status workflows/jade-symphony.md --json
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
value may be a display label such as `Manual Gemini Review`; use the claim
command so Jade Symphony can quote, escape, and validate the stored pointer
before Project mutation.

If Gemini cannot start, the Agent Review timeline comment should name the
configured command, whether worker `PATH` could resolve it, the required
operator action, and the retry command. Do not move an issue to `Human Review`
unless the Review Agent actually records passing review evidence.
If the linked PR is still draft, do not run normal review. Record invalid
handoff evidence and send the work back to Main/operator repair; `doctor repair
<issue> --mark-pr-ready --confirm-handoff-ready --write` is the explicit repair
path when the operator has confirmed the handoff is otherwise complete.

If Gemini exits, refuses the workspace trust check, times out, or produces output
that is not yet parsed into durable pass/finding evidence, the issue must stay
out of `Human Review`. Inspect the recorded tmux attach command, prompt
artifact, session registry entry, and log path, then route with `review pass` or
`review reject` only after independent review evidence exists.

If Gemini returns successfully but says it could not inspect the PR, workspace,
diff, code changes, or required handoff evidence, treat that as an automatic
Review Agent inconclusive result, not a pass. `review loop` records the
inconclusive reason in the ledger/timeline comment and routes the issue to
`Rework` so the missing evidence can be repaired before another independent
review pass.

Manual Gemini or operator-supplied review notes must be routed through
`review pass` or `review reject`, which wraps the note in a
`## Jade Symphony Agent Review Run` timeline comment. Mark the inner note as
manual evidence so operators can distinguish it from automatic `review loop`
pass evidence.

Use `workflows/jade-symphony.md` for supervised review workers. Do not keep the
active review workflow only under `/tmp` or `/private/tmp`; the CLI prints
`workflow_warning=temporary_path` for those workflow files so operators can
promote reusable config into the repo.

## Local Skill Suite

Jade Symphony's local operator skills are repo-owned under
`skills/jade-symphony/` and versioned by `skills/jade-symphony/manifest.toml`.
Use the installer to preview, install, update, or validate the Codex and Gemini
copies instead of hand-copying skill files:

```bash
node scripts/install-jade-symphony-skills.js --dry-run
node scripts/install-jade-symphony-skills.js
node scripts/install-jade-symphony-skills.js --validate
```

The install path is interactive by default. It shows the detected Codex and
Gemini target directories and requires operator confirmation before writing.
Use `--codex-dir`, `--gemini-dir`, `--skip-codex`, or `--skip-gemini` to make
the target set explicit. Use `--yes` only after the printed target paths are
known and intentional.

Skills are per-repo rendered installs, not one global generic skill set. Before
starting a lane that depends on local skills, inspect readiness without writing:

```bash
cargo run -- skills status workflows/jade-symphony.md
cargo run -- skills status workflows/jade-symphony.md --json
cargo run -- skills status workflows/jade-symphony.md --session-skills-file /path/to/session-skills.txt
```

`skills status` discovers the source suite from `--suite-path`,
`JADE_SYMPHONY_SKILL_SUITE`, the current repo's `skills/jade-symphony/suite`,
then installed-only mode when no source suite exists. It reports expected/source
skills, Codex installs, Gemini installs when configured or discoverable,
rendered metadata freshness, broken links, alias/file-shaped installs, missing
`SKILL.md`, and optional current-session visibility. If no session skill input
is provided, session visibility is `unknown`; that is diagnostic context, not a
failure. Gemini absence is not a failure unless the operator explicitly requires
Gemini for the current environment.

The packaged skills preserve the same lane boundaries as the Jade Symphony CLI:
Issue Forge, Reflect, and Dream handle conversation, draft shaping, backlog
mining, and promotion discussion, including Human Review -> Rework revision
discussion; the CLI owns `forge create`, `forge promote`, `forge rework`, and
`forge validate`. Manual Main stops at `Agent Review`; Manual Review owns
evidence-backed review routing; Human Review briefs the operator for UAT and
final acceptance but waits for explicit confirmation before any state change;
Manual Merge owns approved merge-lane work. `doctor` reports read-only local
install-health warnings and points operators back to the #242 install/update
path rather than repairing skill files itself.

## Inspect And Resume

```bash
target/debug/jade-symphony project inspect workflows/jade-symphony.md '#<issue>'
target/debug/jade-symphony project state workflows/jade-symphony.md
target/debug/jade-symphony project issue workflows/jade-symphony.md '#235' --json
target/debug/jade-symphony debug workflows/jade-symphony.md
target/debug/jade-symphony project state workflows/jade-symphony.md --display tui
target/debug/jade-symphony doctor workflows/jade-symphony.md --display tui
target/debug/jade-symphony main loop workflows/jade-symphony.md --max-iterations 1 --write
```

Use `project state` before claiming work when multiple operators are active. A
healthy read prints `project_state_access=ok`, `trusted=true`, the issue count,
and a state summary, plus a read-only `canonical_checkout` cleanliness line for
the launch checkout. A failed read prints `project_state_access=blocked`,
`trusted=false`, and a `failure_kind` such as `auth`, `network`, `rate_limit`,
`transient_backend`, `resource_limit`, `schema`, `partial_response`, `payload`,
or `missing_capability`; treat that as a blocker, not as an empty queue. HTTP
502, 503, and 504 failures are `transient_backend` and retry with bounded
backoff rather than being treated as owner/configuration failures.
This is a queue scan surface: it keeps lane-safe status, claim, assignee,
priority, dependency, and parent/subissue gate data while avoiding issue bodies,
comment/workpad streams, and rich linked-PR hydration.

The canonical checkout is only the harness launch directory. Do not use it as a
Main, Review, or Merge issue worktree, and do not leave runtime state, logs,
prompts, drafts, or evidence there. `main loop --write`, `review loop --write`,
and `merge loop --write` check the launch checkout before tracker mutation:
tracked dirty files block the lane, recognized local artifacts are moved to the
artifact quarantine with a warning, and unclassified untracked files block until
the operator moves them to an issue worktree or artifact location.

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
and the parent issue remains the final Human Review unit. `doctor` now reports
read-only topology blockers for native subissue PRs targeting `main`, missing or
ambiguous parent integration branch evidence, `Done` subissues without merge
evidence into the parent branch, and parent `Human Review` before all native
subissues are `Done` and merged.

Lane handoff and merge flows must make branch target evidence explicit. A
subissue keeps its normal `feature/issue-*` head branch but uses the parent
integration branch as the PR base. A parent final PR uses the parent integration
branch as its head and `main` as its base. Workpads and PR bodies should record
the native parent issue, `parent_integration_branch`, PR base branch, and parent
final base branch when applicable so Review, Doctor, and Merge read the same
topology evidence.

## Issue Forge Reflect

Issue Forge Reflect is a Codex skill workflow, not a Jade Symphony CLI
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
as an append-only `Jade Symphony Rework Run` timeline comment, and sets
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
when the operator wants to sleep on broader Jade Symphony history: recent
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
`.codex/skills/jade-symphony-doctor/SKILL.md` when an operator-selected issue or
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

For a manual Gemini/operator review, claim and route through the CLI:

```bash
target/debug/jade-symphony review claim workflows/jade-symphony.md '#226' --worker "Manual Gemini Review" --write
target/debug/jade-symphony review pass workflows/jade-symphony.md '#226' --evidence-file /tmp/review-evidence.md --write
target/debug/jade-symphony review reject workflows/jade-symphony.md '#226' --evidence-file /tmp/review-evidence.md --target-state rework --write
```

The evidence file for `review pass` or `review reject` must include the exact
structured `Review Agent` claim from `review claim`. `review pass` writes an
append-only Agent Review timeline comment with the review pass marker before
moving to `Human Review`; `review reject` refuses
`Human Review` and may route only to `Agent Review`, `Rework`, or
`Need Human Input`. Both commands preserve the `Review Agent` field as terminal
audit evidence instead of clearing it.

## Artifact Root Migration

To move local runtime artifacts without changing repo-owned workflow files, set
one environment variable before launching dogfood commands:

```bash
export JADE_SYMPHONY_ARTIFACT_ROOT="$HOME/.jade-symphony/artifacts"
```

The live operator workflow derives implementation and review worktree/log paths
from that root. Existing temp Markdown files should be classified before
cleanup: normal operator workflow config belongs in `workflows/`, fixtures and
reference examples belong in `examples/`, reusable operator prompts belong in
`docs/`, issue and PR drafts belong in tracker/workpad or log artifacts, and
disposable scratch can be removed only through a separate cleanup decision.

Use the grouped `clean` surface for local cleanup and persistence questions:

```bash
target/debug/jade-symphony clean plan workflows/jade-symphony.md
target/debug/jade-symphony clean audit workflows/jade-symphony.md
```

`clean plan` is the grouped form of the existing read-only cleanup plan, while
`clean audit` classifies configured artifact/workspace residue as
`promote_to_repo`, `attach_to_tracker`, `safe_to_keep`, `cleanup_candidate`, or
`needs_human_decision`. Keep `doctor` for tracker/runtime invariants and stuck
workflow states.

Interrupted tmux recovery flow:

1. Run `target/debug/jade-symphony status workflows/jade-symphony.md` and read
   the `tmux sessions` section for the session status, attach command, and log.
2. Run `target/debug/jade-symphony doctor workflows/jade-symphony.md` before
   retrying or clearing runtime state; stale, failed, usage-limited, or
   unattributed sessions require operator inspection.
3. For interrupted Main-lane tmux work where the issue is still `In Progress`,
   run a bounded recovery tick:

```bash
target/debug/jade-symphony main loop workflows/jade-symphony.md --max-iterations 1 --max-concurrent 3 --write
```

`main loop --write` restarts recoverable Main runtime slots as new attempts by
default without moving the issue to `Rework`, clearing dirty worktrees, or
advancing to `Agent Review`. It reuses a tracker/runtime/discovery-backed git
worktree under the configured workspace root and leaves normal handoff to a
later successful Main result. Use `--no-recover` only for debugging or a
deliberately conservative operator pass.
4. For interrupted Merge-lane loop work where the issue is still `Merging`, run
   a bounded recovery tick:

```bash
target/debug/jade-symphony merge loop workflows/jade-symphony.md --max-iterations 1 --max-concurrent 2 --write
```

`merge loop --write` adopts interrupted structured merge-loop/goal claims first
by default, then continues normal merge selection. It leaves manual claims
alone, keeps safe stale-base refreshes or merge-lane repairs in `Merging`, and
routes serious blockers to `Need Human Input` rather than `Rework`. Use
`--no-recover` only for debugging or a deliberately conservative operator pass.

5. Run `target/debug/jade-symphony clean audit workflows/jade-symphony.md` only
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
values use `v=1 lane=<main|review|merge> actor=<codex|gemini|claude|human>
worker=<worker> source=<loop|manual|goal> issue=#N run=<id>
state=<active|done|stale|failed|superseded> thread=<codex-link|unknown>
registry=run/<id>`. Keep full paths and terminal logs in the session registry
or workpad, and update terminal completed work to `state=done` instead of
clearing useful claim evidence by default. Display labels with spaces are stored
with reversible quoting, for example `worker="Codex Manual Main"`; raw Project
field edits are a break-glass repair path, not normal claim ownership.
For supervised merge terminals, use `merge claim WORKFLOW '#issue' --worker
<worker> --write` on a `Merging` issue; the claim records truthful non-tmux
manual evidence for the `run=`. Then use `session start WORKFLOW '#issue'
--lane merge --run <RUN_ID> --write` when a supervised tmux terminal is needed.
Session startup validates the `Merging Agent` field and writes attach/log
evidence without merging the PR or closing the issue.

Operator commands also print compact `Latest:` lines for the current lane,
issue, category, action, actor, workspace/branch when known, and next expected
step. Treat these as the glanceable status bar; detailed line logs and JSONL
events remain the durable audit trail.

```bash
target/debug/jade-symphony main loop workflows/jade-symphony.md --max-iterations 1 --max-concurrent 2 --dry-run
target/debug/jade-symphony merge loop workflows/jade-symphony.md --max-iterations 1 --max-concurrent 2 --dry-run
```

## Logical Actor Audit

Dogfood can run many local workers through the same GitHub account. GitHub will
show the configured account for API mutations, so Jade Symphony also writes a
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
target/debug/jade-symphony clean plan workflows/jade-symphony.md
target/debug/jade-symphony clean audit workflows/jade-symphony.md
target/debug/jade-symphony clean plan workflows/jade-symphony.md
```

`clean plan` reports terminal worktrees that appear removable only when tracker
state is terminal, the linked PR is merged or closed, the local worktree branch
matches the issue branch, and the worktree is clean. `clean plan` remains a
compatibility path for the same read-only behavior.

`clean audit` classifies local artifact and workspace residue by persistence
need. It never deletes files.

Do not use this launcher to bypass Agent Review, Human Review, or Merging role
boundaries.
