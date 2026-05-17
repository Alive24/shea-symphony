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
- the workflow validates.
- in write mode, the controlled dogfood smoke preflight passes.

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
target/debug/jade-symphony run-loop workflows/jade-symphony.md --max-iterations 1 --dry-run
```

For a more scannable operator view, keep the same dry-run boundary and opt into
the terminal panel:

```bash
target/debug/jade-symphony run-loop workflows/jade-symphony.md --max-iterations 1 --dry-run --display tui
```

The panel view is not a full-screen dashboard. It keeps plain text and JSON/log
evidence available by default, and only changes output when `--display tui` is
passed. The same opt-in display flag is available on `project-state` and
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

Write mode is intentionally bounded. It runs one `run-loop` tick only after the
explicit confirmation flag is present. Before that mutating tick, the launcher
runs:

```bash
target/debug/jade-symphony project-state workflows/jade-symphony.md
target/debug/jade-symphony run-loop workflows/jade-symphony.md --max-iterations 1 --dry-run
```

If the normal preflight surfaces fail, the launcher exits before claiming
tracker work. `dogfood-smoke` remains a hidden legacy smoke helper for older
fixtures; it is not the canonical operator entrypoint.
The canonical workflow now uses the local `tmux` main-agent backend. A write
tick starts an attachable tmux session, records its attach command and log path,
persists a session registry record under the configured artifact root, and
leaves the issue active until real implementation/handoff evidence exists.
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
other handoff evidence is valid, `run-loop --write` may run `gh pr ready`
before moving the issue to `Agent Review`; if relationship verification or
readiness mutation fails, keep the issue out of `Agent Review`, route to
`Need Human Input`, and preserve the blocker in the workpad.
Routine status output reads the durable session registry, probes bounded pane
and log tails, and reports a conservative session classification such as
`running`, `waiting_for_trust`, `waiting_for_approval`, `usage_limited`,
`failed`, `completed`, `stale`, or `unknown`. The status surface includes only
compact evidence snippets plus attach/log locations; attach manually when raw
scrollback is needed.
`doctor` reads the same registry and reports stale, orphaned, or attention
requiring sessions next to tracker/runtime findings. `clean audit` treats the
session registry, rendered prompts, and tmux logs as recovery evidence, and only
classifies completed sessions as cleanup candidates.
If an operator switches the workflow back to `agent.backend: dry-run`, the
mutating tick exits before runtime-state writes, worktree creation, Project
claims, or workpad mutation.

## Review Backend Setup

For live Agent Review, make the Gemini command visible to the worker process.
`review.gemini_command` is launched directly, so `gemini` is resolved from the
worker `PATH`, not from an interactive shell profile.

Prefer an absolute path when supervising review workers:

```bash
command -v gemini
```

Then configure the workflow or operator environment with that path before
running review automation:

```yaml
review:
  backend: gemini-cli
  gemini_command: /opt/homebrew/bin/gemini
```

```bash
export GEMINI_CLI_TRUST_WORKSPACE=true
target/debug/jade-symphony review loop workflows/jade-symphony.md --max-iterations 1 --write
```

For supervised manual review terminals, first use
`review claim WORKFLOW '#issue' --worker <worker> --write` on an `Agent Review`
issue, then start the runtime with `session start WORKFLOW '#issue' --lane
review --run <RUN_ID> --write`. Session startup validates the existing Review
Agent claim and writes attach/log evidence without moving the issue to
`Human Review`.

If Gemini cannot start, the review workpad should name the configured command,
whether worker `PATH` could resolve it, the required operator action, and the
retry command. Do not move an issue to `Human Review` unless the Review Agent
actually records passing review evidence.
If the linked PR is still draft, do not run normal review. Record invalid
handoff evidence and send the work back to Main/operator repair; `doctor repair
<issue> --mark-pr-ready --confirm-handoff-ready --write` is the explicit repair
path when the operator has confirmed the handoff is otherwise complete.

If Gemini exits, refuses the workspace trust check, or times out before
returning a review report, `review loop` records terminal workpad/ledger
evidence, clears the `Review Agent` Project claim, and leaves the issue in
`Agent Review` for retry after the operator fixes the backend environment.
The Gemini subprocess receives the rendered prompt on stdin and Jade Symphony closes
stdin after writing so headless commands that wait for EOF can proceed.

If Gemini returns successfully but says it could not inspect the PR, workspace,
diff, code changes, or required handoff evidence, treat that as an automatic
Review Agent inconclusive result, not a pass. `review loop` records the
inconclusive reason in the ledger/workpad and routes the issue to `Rework` so
the missing evidence can be repaired before another independent review pass.

Manual Gemini or operator-supplied review notes must use an explicit manual
evidence marker such as `## Manual Agent Review Evidence`. They are not the same
thing as automatic `review loop` pass evidence and should not be used to satisfy
the automatic Review Agent boundary unless the workflow explicitly says so.

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

The packaged skills preserve the same lane boundaries as the Jade Symphony CLI:
Issue Forge and Reflect handle conversation, draft shaping, and promotion
discussion; the CLI owns `forge create`, `forge promote`, and `forge validate`.
Manual Main stops at `Agent Review`; Manual Review owns evidence-backed review
routing; Manual Merge owns approved merge-lane work. Automatic doctor
install-health checks remain future work for #256.

## Inspect And Resume

```bash
target/debug/jade-symphony inspect workflows/jade-symphony.md
target/debug/jade-symphony project-state workflows/jade-symphony.md
target/debug/jade-symphony project-issue workflows/jade-symphony.md '#235' --json
target/debug/jade-symphony debug workflows/jade-symphony.md
target/debug/jade-symphony project-state workflows/jade-symphony.md --display tui
target/debug/jade-symphony doctor workflows/jade-symphony.md --display tui
target/debug/jade-symphony run-loop workflows/jade-symphony.md --max-iterations 1 --write
```

Use `project-state` before claiming work when multiple operators are active. A
healthy read prints `project_state_access=ok`, `trusted=true`, the issue count,
and a state summary, plus a read-only `canonical_checkout` cleanliness line for
the launch checkout. A failed read prints `project_state_access=blocked`,
`trusted=false`, and a `failure_kind` such as `auth`, `network`, `rate_limit`,
`schema`, `partial_response`, or `payload`; treat that as a blocker, not as an
empty queue.

The canonical checkout is only the harness launch directory. Do not use it as a
Main, Review, or Merge issue worktree, and do not leave runtime state, logs,
prompts, drafts, or evidence there. `run-loop --write`, `review-loop --write`,
and `merge-loop --write` check the launch checkout before tracker mutation:
tracked dirty files block the lane, recognized local artifacts are moved to the
artifact quarantine with a warning, and unclassified untracked files block until
the operator moves them to an issue worktree or artifact location.

Use `project-issue` for per-issue Project status, Project fields, blocker
relationships, claim locks, and linked PRs. Raw `gh issue view` and `gh pr view`
remain acceptable for ordinary issue/PR body text, comments, and diff context,
but normal dogfood should not read or mutate Project fields, status, claim locks,
or relationships through raw Project GraphQL or the Project UI. Those are
break-glass recovery paths.

For parent tracking issues with native GitHub subissues, use
`docs/parent-subissue-topology.md` as the design source. Native sub-issue links
define hierarchy, subissue PRs target the parent integration branch by default,
and the parent issue remains the final Human Review unit.

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

Use `debug` when you need one read-only operator report before a supervised
dogfood, repair, review, or merge session. It summarizes the current Project
queue, doctor health, smoke readiness, runtime/session state, cleanup/audit
status, and lane-specific next commands without claiming work, starting workers,
repairing state, cleaning artifacts, or implying unattended readiness.

Use the repo-owned Doctor skill at
`.codex/skills/jade-symphony-doctor/SKILL.md` when an operator-selected issue or
`Need Human Input` item needs triage before normal lane work can resume. The
skill is read-first: it gathers `project-state`, `doctor`, `debug`, and
`project-issue` evidence, classifies the stuck state, and produces a structured
`Doctor Triage Note` with any repair actions that still require explicit
confirmation. Keep full local skill install checking in #256 and dated
installable skill suite packaging in #242.

If `run-loop` finds runtime-state for an issue that has already moved out of
active main-agent work, it reconciles tracker state first. Clean or absent
workspaces are archived under the configured runtime log directory and the loop
continues; dirty or unknown workspaces still stop the loop with a repair
diagnostic so local work is not discarded silently.

`doctor` treats Human Review as valid only when independent review pass evidence
is durable. Project fields named `review_pass_evidence_recorded` or
`review_pass_evidence` satisfy that check when a tracker exposes them; in the
current GitHub Project #9 schema, the canonical source is the review workpad text
written into the issue comment stream. A `Review Agent` claim by itself is not
pass evidence.

For a manual Gemini/operator review, claim and route through the CLI:

```bash
target/debug/jade-symphony review claim workflows/jade-symphony.md '#226' --worker "Manual Gemini Review" --write
target/debug/jade-symphony review pass workflows/jade-symphony.md '#226' --evidence-file /tmp/review-evidence.md --write
target/debug/jade-symphony review reject workflows/jade-symphony.md '#226' --evidence-file /tmp/review-evidence.md --target-state rework --write
```

The evidence file for `review pass` or `review reject` must include the exact
structured `Review Agent` claim from `review claim`. `review pass` writes the
review pass marker before moving to `Human Review`; `review reject` refuses
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
3. Run `target/debug/jade-symphony clean audit workflows/jade-symphony.md` only
   after evidence is preserved. Active or uncertain sessions stay
   `needs_human_decision`; completed sessions and terminal clean worktrees may
   become cleanup candidates.

For supervised parallel operators, pass `--pool N` to preview eligible slots and
apply lane-specific claim checks. Main work uses the `Main Agent` Project field
as a soft claim-lock hint while still processing one active runtime issue per
loop tick. Merge work uses the `Merging Agent` Project field and can process
multiple guarded merge slots in one bounded loop.
Lane claim fields are latest-run audit pointers, not append-only logs. New
values use `v=1 lane=<main|review|merge> actor=<codex|gemini|claude|human>
worker=<worker> source=<loop|manual|goal> issue=#N run=<id>
state=<active|done|stale|failed|superseded> thread=<codex-link|unknown>
registry=run/<id>`. Keep full paths and terminal logs in the session registry
or workpad, and update terminal completed work to `state=done` instead of
clearing useful claim evidence by default.
For supervised merge terminals, use `merge claim WORKFLOW '#issue' --worker
<worker> --write` on a `Merging` issue, then `session start WORKFLOW '#issue'
--lane merge --run <RUN_ID> --write`. Session startup validates the `Merging
Agent` field and writes attach/log evidence without merging the PR or closing
the issue.

Operator commands also print compact `Latest:` lines for the current lane,
issue, category, action, actor, workspace/branch when known, and next expected
step. Treat these as the glanceable status bar; detailed line logs and JSONL
events remain the durable audit trail.

```bash
target/debug/jade-symphony run-loop workflows/jade-symphony.md --max-iterations 1 --pool 2 --dry-run
target/debug/jade-symphony merge-loop workflows/jade-symphony.md --max-iterations 1 --pool 2 --dry-run
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
target/debug/jade-symphony cleanup-plan workflows/jade-symphony.md
```

`clean plan` reports terminal worktrees that appear removable only when tracker
state is terminal, the linked PR is merged or closed, the local worktree branch
matches the issue branch, and the worktree is clean. `cleanup-plan` remains a
compatibility path for the same read-only behavior.

`clean audit` classifies local artifact and workspace residue by persistence
need. It never deletes files.

Do not use this launcher to bypass Agent Review, Human Review, or Merging role
boundaries.
