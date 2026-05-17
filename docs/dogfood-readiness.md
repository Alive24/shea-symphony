# Dogfood Readiness

Status: native tmux supervision dogfood baseline with read-only GitHub Project v2
loading through the `gh` CLI and a `run-loop` skeleton that can launch
attachable lane sessions without treating session creation as completion.
The live GitHub Project workflow now carries a real Jade Symphony operating prompt, but
Jade Symphony is not ready for unattended live GitHub Project v2 execution yet.

## Current Capability Status

| Capability | Current Status |
| --- | --- |
| Workflow loader | Implemented for explicit workflow path and optional YAML front matter. A first-slice `WorkflowStore` can explicitly reload a workflow while preserving the last known good definition after load/parse failures. Long-running runtime reload wiring is not implemented. |
| Dogfood workflow prompt | `workflows/jade-symphony.md` is the canonical normal operator workflow index/config and references `workflows/prompts/main-agent.md`, `workflows/prompts/review-agent.md`, and `workflows/prompts/merge-agent.md`. Main, Review, and Merge initialization load the lane-specific prompt contract; older fixture workflows remain compatible with inline prompt bodies. Tests guard against reverting the Main Agent prompt to a placeholder-thin contract. |
| Typed config | Implemented for the current skeleton, including GitHub Project v2-shaped settings, operator/agent identity metadata, first-slice execution profiles, and optional SSH worker host settings for future remote scheduling. |
| Normalized issue model | Implemented as `TrackerIssue`, including ProjectV2 item ID, labels, assignees, blockers, linked PRs, and project fields. |
| GitHub Project v2 tracker | Fixture-backed mode plus live loading and explicit write operations through `gh api graphql`. `run-loop` can coordinate existing primitives, idle-poll in unbounded write mode, and use claim decision helpers before write-mode dispatch; auth diagnostics distinguish fixture mode, env-token auth, usable `gh api graphql` auth, missing `gh`, and unusable auth. Same-state status writes are skipped, Project item addition initializes the configured `Status` field to `Todo`, marker workpad upsert is idempotent for the canonical marker, and claim decision helpers distinguish claimable, active, and externally changed states. Full claim reconciliation is not implemented yet. |
| Linear tracker | Implemented behind the same trait for fixture-backed planning plus live GraphQL reads, state updates, marker workpad comments, follow-up issue creation, and project assignment. Credential-gated smoke coverage is still missing. |
| Issue Quality Gate | Implemented as a Markdown contract check plus deterministic source-alignment preflight where workflow/repo context is available. It verifies the template `UAT Required` field, relationship-first dependency semantics, target repository, referenced local paths, and supported verification command shapes. Structured `blocked_by` tracker relationships are authoritative for dispatch blocking; missing Markdown dependency boilerplate no longer blocks otherwise independent work, while body-only blocker claims still require clarification or a structured relationship. Optional local command-backed LLM gate mode exists as disabled/advisory/required; richer semantic validation and hosted providers are still follow-ups. |
| Issue Forge | The Jade Symphony CLI exposes a compact `forge` command group: `forge validate`, `forge create`, and `forge promote`. Conversation, reflection, and repair are owned by Codex skills. `forge create --status Backlog` uses a lighter Backlog seed gate, `forge create --status Todo` uses the full Issue Quality Gate, creation always adds the issue to the configured Project, and live `Todo` creation requires `--assignee` before adding executable Project items. `forge promote` updates an existing Backlog issue in place with explicit dry-run and failure-checkpoint reporting, and now owns the structured Promotion Note comment requirement. Text/number/date field setup is not implemented yet. |
| Orchestrator | Deterministic dispatch planning and a CLI `run-loop` skeleton with bounded modes, idle polling, claim-helper use, dependency preflight for `Todo` / `Rework`, runtime-state persistence, tracker-visible advisory ownership markers, resume preflight, retry backoff records, stall detection, live PR handoff plus tracker PR link recording in non-fixture GitHub mode, ready/non-draft PR enforcement before Agent Review handoff, guarded `merge-once` and bounded `merge-loop` lanes for `Merging` issues, first-slice `--pool` selection guarded by `Main Agent` / `Merging Agent` Project fields, a controlled `dogfood-smoke` preflight report with blocking-vs-warning integration gap severity, and a bounded operator launcher script exist. No long-running worker supervision, automated stall restart, full multi-worker runtime resume reconciliation, unbounded merge idle polling, or full state reconciliation yet. |
| Workspace | Local path sanitization, creation, timeout-aware hooks, stdout/stderr capture, `before_remove`, safe cleanup helpers, grouped read-only `clean plan` / `clean audit` cleanup and persistence classification, guarded terminal cleanup planning, repository-local git identity application, workspace/branch/PR handoff planning, live git worktree/branch creation, issue-level `workspace list` / `workspace show` / `workspace adopt` discovery across registry, workpad, PR, and local git worktree evidence, dirty/no-op guards before branch push, optional configured verification commands before PR handoff, branch push, PR create-or-reuse, tracker PR link recording, run-loop handoff evidence, profile-scoped workspace keys, parsed-but-unused SSH worker host config, a namespaced artifact layout, and dry-run cleanup planning exist. Automatic runtime cleanup, write-mode artifact cleanup, and live SSH execution are not wired yet. |
| Execution profiles | First-slice profile discovery exists. Workflow config can point to a cockpit-tools Codex `codex_instances.json` file, and Jade Symphony treats each instance `name` as a profile/worker identity while ignoring account binding fields. If the cockpit-tools file is missing, explicit `profiles.entries` are used. This is not a full account manager. |
| Agent backends | Dry-run backend plus conservative Codex and Claude Code subprocess backends exist. A local `tmux` backend can launch attachable lane sessions, capture the Codex pane before prompt injection, auto-advance the Codex workspace trust prompt inside Jade Symphony-created issue worktrees by default, paste rendered prompts only after readiness is observed, record the session name, attach command, prompt artifact, log path, lane, actor, profile/instance metadata, and durable session registry record, and leave workflow state unchanged until normal lane evidence is ready. `run-loop` uses that backend for Main Agent execution, while manual recovery now claims with `main claim`, `review claim`, or `merge claim` before starting runtime with `session start --run <RUN_ID>`. `status` classifies recent registered sessions from bounded pane/log evidence as `starting`, `running`, `waiting_for_trust`, `waiting_for_approval`, `waiting_for_human_input`, `usage_limited`, `failed`, `completed`, `stale`, or `unknown`. `JADE_SYMPHONY_TMUX_AUTO_TRUST=0` opts out and fails closed if the trust prompt is visible. Prepared runs include selected profile/instance metadata and profile environment context. The Codex subprocess backend safely refuses `codex app-server` commands because that transport is not implemented yet. A first-slice Codex app-server event normalizer maps fixture JSON-RPC stream lines into Jade Symphony `AgentEvent` values, including completion, failure, cancellation, input-required, token usage, notification, and malformed events. Full Codex app-server transport and Claude Code protocol parity remain follow-ups. |
| Prompt rendering | Strict prompt rendering supports `issue.*`, `attempt`, and basic `{% if %}` / `{% else %}` blocks. The supported subset is documented in `docs/prompt-template-contract.md`; full Liquid compatibility remains a parity gap. |
| Dynamic tools | A first-slice dynamic-tool registry can describe planned backend-specific tools such as Codex `linear_graphql` without coupling them to the orchestrator. Tool execution and Codex app-server dynamic-tool protocol wiring are not implemented yet. |
| Agent Review | Finding classes, fake reviewer lifecycle, Gemini CLI subprocess backend, role-bound transition decisions, evidence-first Rework diagnostics for confirmed findings and completed-but-inconclusive automatic reviews, invalid-handoff evidence when Agent Review receives a draft PR, `review-freshness` evidence for Merging conflict repair, bounded `review-loop` worker selection/reconciliation with one issue per worker slot, Project `Review Agent` claim markers before backend launch, terminal failure/timeout claim cleanup for retry, terminal manual review claim preservation, durable JSON review job ledger records, and workpad/status evidence helpers exist. Persistent background review worker supervision is not implemented yet. |
| Merging | `merge-once` can inspect issues already in `Merging`, require one linked PR, treat Project `Merging` as the approval signal, derive fixture merge preflight status from linked PR metadata for dry-run rehearsal, check live GitHub PR state/review/check/mergeability data where available, recheck transient missing or `UNKNOWN` mergeability once before deciding, merge clean PRs only with explicit `--write`, record workpad evidence, set Project `Done`, close the linked GitHub issue when supported by the tracker, route dirty/failing blockers to `Rework`, leave transient pending/unknown cases in `Merging` for retry, and only route to `Need Human Input` with a concrete workpad question. Bounded `merge-loop` can repeat that guarded tick for an explicit iteration count and `--pool N` can process multiple guarded merge slots while skipping issues claimed by another `Merging Agent`. Unbounded continuous merge polling is not implemented yet. |
| Project doctor | Read-only `doctor` / `audit-project` reports workflow invariant violations from normalized tracker issues, including missing PR handoff evidence, Agent Review draft PRs, missing review pass evidence, dirty Merging PRs, ambiguous issue worktree candidates, missing runtime ownership hints, queued issues with PRs, stale registered tmux sessions, orphaned/unattributed session registry entries, session/runtime mismatches, and sessions needing operator attention. Project Status `Done` issues are skipped for issue-internal operational checks such as lane claims, runtime ownership, and session state, but tracker terminal-state mismatches between GitHub issue open/closed state and Project `Done` status still report. Human Review pass evidence can come from Project fields when present or from the canonical review workpad text written into the issue description/comment stream; Project #9 does not require a dedicated manual review pass field. JSON output and strict blocker failure modes exist. `doctor repair <issue> --mark-pr-ready --confirm-handoff-ready --write` can repair draft PR handoffs only with explicit operator confirmation, and `doctor-repair-human-review` can repair the specific invalid Human Review-without-pass-evidence case with explicit `--write`; broader repair mode is not implemented yet. The repo-owned Doctor skill at `.codex/skills/jade-symphony-doctor/SKILL.md` now defines the read-first triage path for `Need Human Input` and operator-selected issue diagnosis, including a structured `Doctor Triage Note` and explicit relation to #256 and #242. |
| Observability | Operator-readable terminal snapshots report polling, running, retrying, skipped issues, gate details, token counters, event-log path, integration gaps, latest-status bars, dry-run cleanup candidates, and recent registered tmux session summaries with status, evidence source, attach command, and log path. The read-only `debug` command composes Project, doctor, smoke readiness, runtime/session, cleanup, and lane next-action signals into one human report without claiming work or implying unattended readiness. `run-loop`, `review-loop`, and `merge-once` emit compact `Latest:` lines for current lane/action/category while preserving detailed logs. `run-loop`, `project-state`, and `doctor` also support an opt-in `--display tui` panel view backed by the Codex-aligned `ratatui` / `crossterm` stack while preserving default line output for logs and scripts. `status WORKFLOW --json` prints the same `RuntimeSnapshot` shape for dogfood scripts, and `status-api WORKFLOW --once` serves that snapshot on local `GET /status.json` / `GET /status` for one request. JSONL event-log primitives exist, can read back records, and can produce compact summaries by event, issue, and session; `run-once` writes dry-run events with actor and profile metadata. `tracker_mutation` audit records now capture logical actor role/label, command, mutation type, issue, target, from/to state when known, reason, and timestamp for state changes, claim field writes, workpad writes, PR links, issue/project creation, review routing, merge routing, and repair actions. `TokenTotals::from_agent_events` provides a first aggregation helper for normalized token events, but live runtime/app-server wiring is still pending. Runtime state files are written during write-mode `run-loop` issue execution, including actor role/label, git author, and optional profile/instance identity when configured; resume, retry, usage-limit pause, stall supervision events, and tmux session registry probes are also recorded or surfaced. Repo-owned live implementation and Gemini review workflows default durable roots under `~/.jade-symphony/artifacts`, support `JADE_SYMPHONY_ARTIFACT_ROOT`, and warn when an operator points commands at temp-only workflow files. Persistent/remote web/API service mode is not implemented yet. |
| Usage-limit pause/resume | Conservative usage-limit/rate-limit/resource-exhausted classification exists for subprocess agent events and review job output. `run-loop` records usage-limit pauses in workpad/runtime retry state and does not advance to `Agent Review`; review workpads surface usage-limit failures without moving to `Human Review`. Vendor-specific quota management is not implemented. |
| Tests | Unit tests cover the dry-run skeleton. Read-only / dry-run live GitHub smoke tests exist behind explicit `JADE_LIVE_GITHUB_SMOKE=1` opt-in; mutation and Linear credential-gated smoke coverage are still missing. |

The operator launcher runbook is in `docs/operator-dogfood.md`; it keeps write
mode explicit through `scripts/jade-dogfood --write --confirm-write` and runs
the controlled dogfood smoke preflight before a mutating tick.

The live dogfood workflow definition lives in `workflows/jade-symphony.md`.
Temp Markdown files can still be useful for drafts, but reusable workflow config
and operator prompts should be promoted into `workflows/`, `examples/`, or
`docs/` before they become the canonical run path. Normal operator workflow
config belongs in `workflows/`; `examples/` is fixture/reference material.

Normal dogfood should exercise `project-state`, `run-loop`, `review loop`,
`merge-loop`, `doctor`, and related lane preflight surfaces directly.
`docs/dogfood-smoke.md` and `examples/dogfood-smoke-workflow.md` remain legacy
fixture references for the older controlled smoke helper, not the canonical
operator entrypoint.

The bootstrap completion audit is in `docs/bootstrap-parity-audit.md`. It
separates landed mainline capability from open `Agent Review` coverage and
deferred parity work.

## Native tmux Supervision Contract

Jade Symphony's tmux layer is now an operator-supervised terminal substrate owned by the
Jade Symphony runtime, not a second workflow engine. The tracker remains authoritative
for issue lifecycle, while the session registry and logs provide durable local
evidence for attach, diagnosis, and cleanup.

The landed supervision slices are:

- #225 / PR #233: durable session registry and deterministic naming contract.
- #226 / PR #236: pane capture, log-tail probing, and conservative status
  classification.
- #227 / PR #237: lane coverage for Main, Review, and Merging prompts through
  shared session plumbing plus lane-specific shortcuts.
- #228 / PR #239: `status`, `doctor`, and `clean audit` integration for session
  summaries, stale/orphan/mismatch diagnosis, and read-only artifact
  classification.

The resulting operator loop is:

1. Start or resume a bounded lane session with `run-loop --write`,
   or manually claim with `main claim`, `review claim`, or `merge claim`, then
   start runtime with `session start --lane main|review|merge --run <RUN_ID>`.
2. Use `status` / `status-api` to inspect compact session state, attach command,
   log path, and evidence source without treating a running session as
   completed work.
3. Use `doctor` when tracker state, runtime state, or session registry evidence
   appears stale, missing, orphaned, or mismatched.
4. Use `clean audit` to classify registry files, rendered prompts, tmux logs,
   session records, and terminal worktrees before any deletion or archival.

Non-goals remain unchanged: tmux status does not approve reviews, merge PRs,
close issues, replace Project state, or discard worktree/log evidence.

## Before Executing GitHub Project Issues

These must exist before Jade Symphony can safely dogfood against real GitHub
Project v2 issues:

1. Harden read-only GitHub Project v2 adapter.
   - Keep loading ProjectV2 items through `gh api graphql` or replace it with a
     direct HTTP client behind the same adapter.
   - Use `project-state` as the canonical dogfood diagnostic before claim or
     merge work; failed reads print a classified blocker instead of looking like
     an empty queue.
   - Use `project-issue` for per-issue Project status, fields, claim locks,
     blockers, and linked PRs. Direct `gh issue view` / `gh pr view` is still
     allowed for raw issue and PR context, but not for normal Project state
     reads.
   - Retry transient network and rate-limit failures, and fail partial Project
     payloads loudly when required item fields are missing.
   - Filter to real GitHub Issues, not draft items or PR items.
   - Resolve configured status field and option IDs.
   - Normalize issue body, labels, assignees, linked PRs, project fields, and
     timestamps into `TrackerIssue`.
   - Add blocker mapping once the exact GitHub issue relationship source is
     selected.

2. Harden GitHub tracker writes.
   - Current explicit commands can set ProjectV2 status by option ID,
     create/update one marker workpad comment, create follow-up issues, add
     issues to the configured project, claim Main/Review/Merge lane fields, and
     route manual review pass/reject outcomes.
   - Mutating commands require `--write`.
   - Same-state status updates are treated as no-ops before mutation.
   - PR relationship verification is first-class for lane transitions:
     Project/issue linked-PR reads must expose the PR before Main handoff,
     Review routing, or Merge landing. Workpad/comment URLs are discovery
     evidence only; if Jade Symphony can identify a PR but cannot verify or
     repair the relationship, the issue must stop in `Need Human Input`.
   - Remaining work: idempotency checks around project-item addition and richer
     reconciliation after writes.

3. Dispatch safety.
   - Enforce assignee filter from live GitHub issue assignees.
   - Live GitHub `run-loop --write` requires unassigned issue execution to be
     explicitly allowed, and otherwise compares issue assignees against the
     current `gh` login or selected profile login before claim.
   - Live write-mode claim, session, run-loop, review-loop, and merge-loop
     commands require the canonical launch checkout to be attached to latest
     `main`; detached HEAD, non-`main`, or stale `main` states block with
     operator guidance instead of mutating git.
   - `run-loop` reuses tracker claim helpers to claim only `Todo` / `Rework`,
     resume active `In Progress`, and stop/replan on externally changed states.
   - Main-agent dispatch treats structured tracker blockers as authoritative,
     skips `Todo` / `Rework` issues with unresolved blockers before claim, and
     no longer requires Markdown dependency boilerplate for independent work.
   - Revalidate issue state immediately before dispatch.
   - Keep GitHub-specific fields out of `orchestrator`.
- Current explicit `gate-apply` can record quality-gate assumptions or
  missing context in the workpad and move non-dispatchable issues to
  `Need to Clarify` / `Need Human Input`; run-loop pre-dispatch uses the same
  deterministic source-alignment decision before dispatch.

4. Agent Review authority boundary.
   - Main implementation commands can move locally complete work to
     `Agent Review` but cannot set `Human Review`.
   - Independent Review Agent commands can set `Human Review` only after a
     passed review with evidence.
   - Manual/operator review uses `review claim`, `review pass`, and
     `review reject` instead of raw Project field edits. `review pass` writes
     the doctor-recognized review pass marker before `Human Review`;
     `review reject` refuses `Human Review`; both commands require the exact
     current `Review Agent` claim in the evidence file and preserve the field
     as terminal audit evidence.
   - Bounded `review loop` can discover eligible `Agent Review` issues, skip
     existing review-worker markers, select up to the configured concurrent
     worker limit, and apply the same independent Review Agent transition rules
     as `review once`.
   - Write-mode `review loop` records a tracker-visible `Review Agent` marker
     before launching the review backend so parallel review workers can skip an
     already claimed Agent Review item.
   - Terminal review backend failures and timeouts record operator evidence,
     clear stale `Review Agent` claims, and allow retry without manual Project
     field cleanup.
   - Write-mode review jobs persist a JSON ledger record under the configured
     logs root with issue, worker, backend, artifact, decision, summary/error,
     and finding count; review and Rework workpads link to that ledger when
     available.
   - Confirmed findings route to `Rework`.
   - Completed automatic reviews that say the PR, workspace, diff, code changes,
     or handoff evidence could not be inspected route to `Rework` with an
     explicit inconclusive-review ledger/workpad outcome.
   - Failed, timed out, or unavailable reviews remain out of `Human Review` and
     route to `Need Human Input` or stay in `Agent Review`.
   - Merging-to-Rework repairs must record review freshness evidence before
     preserving prior Human Review. Mechanical conflict repair can preserve
     prior Human Review for an authorized merge/handoff flow; semantic or
     unknown rework requires the normal Agent Review and Human Review path.
   - Main-agent completion should enter `Agent Review` only after durable
     handoff evidence includes issue, workspace, branch, validation, transition,
     PR URL, and non-draft PR status. Missing or draft PR evidence should keep
     the issue out of `Agent Review` with a workpad diagnostic.
   - Rework transitions should remain evidence-first: write compact structured
     diagnostics to the canonical issue workpad before setting `Rework`; if the
     diagnostic write fails, stop before changing state.

5. Linear integration hardening.
   - Add credential-gated live smoke tests for reads, state updates, and workpad
     upsert.
   - Confirm `commentUpdate` and project/team lookup against the active Linear
     workspace schema.
   - Keep Linear-specific GraphQL isolated to the adapter.
   - Preserve fixture mode for credential-free development.

6. Live agent execution.
   - The live dogfood workflow prompt now embeds enough Jade Symphony protocol for an
     isolated backend agent to understand the issue work cycle, workpad, review
     boundary, and stop conditions.
   - Keep the prompt aligned with `docs/bootstrap/JADE_WORKFLOW.md` as the
     bootstrap contract evolves.
   - Harden the Codex and Claude Code subprocess backends, then implement full
     protocol transports.
   - Keep refusing `codex app-server` in the subprocess backend until the
     dedicated app-server transport exists.
   - Use the app-server event normalizer as the first protocol boundary for
     future Codex app-server transport wiring.
   - Preserve selected profile identity in prepared runs, logs, runtime state,
     and backend environment context without logging secrets.
   - Keep Claude Code as a peer backend path.
   - Normalize session IDs, completion, failures, token usage, and rate-limit
     events.
   - Ensure external command execution happens only inside the prepared
     workspace.
   - Keep acting identity explicit in backend context and avoid sharing profile
     credentials across concurrent workers.

7. Long-running orchestration.
   - Continuous idle polling exists for unbounded write mode; full active worker
     supervision is still pending.
   - claimed/running/retry state ownership.
   - Runtime state writes exist for claim/resume, backend result evidence, and
     final transition intent. Resume preflight now treats persisted runtime
     state as recovery evidence: it archives and clears stale non-active state
     when the referenced workspace is clean or absent, blocks dirty/unknown
     workspaces with an actionable diagnostic, honors retry backoff, and
     reports active stalls. Full multi-worker reconciliation remains pending.
   - Structured tracker blockers are the dependency source of truth. Missing
     body dependency boilerplate does not block otherwise independent work, but
     semantic dependency placeholders and body-only blocker claims stay in
     `Need to Clarify`; tracker blockers must be terminal before dispatch.
   - Write-mode `run-loop` now writes a tracker-visible runtime ownership marker
     before backend execution and skips active `In Progress` work when the
     marker points at a different profile, workspace key, or branch. `run-loop`
     and `merge-loop` also use the Project text fields `Main Agent` and
     `Merging Agent` as lane-specific claim hints before selecting work. These
     are advisory coordination surfaces, not distributed locks. New lane claims
     use the structured `v=1` key/value audit-pointer format and preserve the
     same `run=` in Project fields, session registry records, prompt context,
     and workpad handoffs.
   - workspace/branch/PR handoff is recorded by `run-loop`; in live
     non-fixture GitHub mode the runtime can create/reuse the issue worktree and
     branch, run optional configured verification commands, push the branch, and
     create/reuse one PR after successful backend execution, while blocking
     dirty or no-op branch handoff. It attempts to record the PR link through
     the tracker adapter, verifies the Project-visible linked-PR read surface,
     and marks draft PRs ready before Agent Review handoff. Remaining work:
     cleanup, richer reconciliation, and richer verification modeling.
   - `cleanup-plan` can report terminal worktree candidates without deleting
     files. Remaining work: explicit write-mode artifact cleanup after operator
     review.
   - Local git identity application exists for prepared git repositories; the
     live worktree path must continue to apply it before commits and preserve the
     distinction between agent actors and human operators.
   - Existing `Agent Review` items with stale, draft, or missing PR evidence
     still need reconciliation/repair; the current handoff invariant prevents
     new silent transitions from passing without ready PR evidence.
   - profile-scoped workspace keys and login-based claim checks exist, but full
     profile-specific account/token switching still needs reconciliation before
     parallel worker dogfooding.
   - continuation retry after normal active-state exits.
   - exponential backoff for failures.
   - stall detection.
   - terminal/non-active reconciliation.
- terminal workspace cleanup planning exists as an explicit dry-run/write
  command; automatic cleanup wiring in the runtime loop remains pending.
- artifact cleanup write-mode wiring in the runtime loop remains pending.

8. Observability.
   - Structured logs for dispatch, retry, state transition, backend session, and
     failure events.
   - Runtime snapshot command exists for dry-run and live-read planning; an API
     endpoint is still needed.
   - Clear integration-gap reporting when credentials are missing or unusable
     while avoiding false missing-token warnings when `gh api graphql` works.
   - `doctor` / `audit-project` is available as a project/workflow/runtime
     invariant audit with human-readable output, JSON output, explicit strict
     failure signaling for blocker violations, repo/default workflow discovery,
     stale runtime-state detection, partial `Todo` claim detection, and
     `In Progress` PR-evidence detection. Human Review pass evidence is
     accepted from explicit Project fields when available or from the canonical
     review workpad marker `Review pass evidence: recorded`; a `Review Agent`
     claim alone is not evidence. `doctor --interactive` and
     `doctor repair ISSUE` provide non-destructive repair guidance, while
     explicit-write repair can move uncertain work to `Need Human Input` after
     writing workpad evidence. `doctor --auto-fix --write` is limited to clearly
     safe repairs such as invalid `Human Review` issues without independent
     Review Agent pass evidence.

9. Integration profile.
   - Credential-gated GitHub Project v2 smoke test.
   - Fixture tests for pagination, status option cache, malformed payloads, and
     permission failures.
   - Dry-run fixtures remain available for local development without credentials.
   - cockpit-tools integration remains a small adapter over the local Codex
     instance store. The inspected source in
     `https://github.com/jlcodes99/cockpit-tools` uses a camelCase `InstanceStore`
     (`instances`, `defaultSettings`) and `InstanceProfile` records with
     `id`, `name`, `userDataDir`, `workingDir`, `extraArgs`, launch metadata,
     and account binding fields. Jade Symphony reads only non-secret instance identity
     and path/argument context.

10. Issue Forge tracker completion.
   - `forge validate` can validate body files or existing issues for `Backlog`
     or `Todo`, using the seed gate for Backlog and the full Issue Quality Gate
     for Todo.
   - `forge create` creates tracker issues, always inserts them into the
     configured Project, supports `--status Backlog|Todo`, and accepts repeatable
     `--project-field NAME=VALUE` assignments.
   - `forge promote` reads a Backlog issue, validates an explicit replacement
     title/body with the Todo gate, requires structured Promotion Note inputs,
     edits the same issue in place, sets Project status to `Todo`, writes the
     Promotion Note comment, and performs readback verification.
   - Reflection, discussion, and repair are skill-owned workflows, not
     Jade Symphony CLI subcommands.
   - Remaining work: text/number/date field writes and richer Project selection
     beyond the configured workflow Project.

## Recommended Next GitHub Issues

Create new Project #9 issues from this list only after checking the live queue
with `gh project item-list`, `project-state`, and `doctor`. Earlier first-slice
read/write, review, merge, workflow, and docs items have landed; do not recreate
that historical backlog unless the live audit shows a regression.

### 1. Turn Dispatch Plan Into Polling Runtime

Goal: evolve the bounded `run-loop` skeleton and `Orchestrator::plan_dispatch`
into a long-running runtime while preserving deterministic planning tests.

Acceptance:

- owns claimed/running/retry state.
- revalidates issue state before dispatch.
- supports global and per-state concurrency.
- schedules continuation and failure retries.
- exposes runtime snapshots.
- runs the Issue Quality Gate before dispatch and routes failed issues through
  the existing workpad/state adapter operations.
- keeps terminal status snapshots linked to the active event log and integration
  gaps.
- uses the runtime state file model to resume active issue, workspace, branch,
  backend session, attempt count, last event, and last transition.
- preserves the current resume preflight guarantees: stale active state must not
  be overwritten, retry backoff must be visible, and stalled work must stop the
  loop safely.

### 1b. Harden LLM-Assisted Issue Quality Gate

Goal: evolve the optional local command-backed LLM gate into a stronger
dogfood path without weakening deterministic checks.

Acceptance:

- add credential-gated or local-model smoke coverage for a real reviewer.
- refine the prompt/output schema from dogfood evidence.
- add hosted-provider adapters only behind explicit configuration.
- preserve required-mode blocking for malformed or unavailable model output.

### 2. Implement Full Codex App-Server Backend

Goal: evolve the conservative Codex subprocess path into full Codex app-server
protocol execution inside the workspace.

Acceptance:

- launches configured Codex command in workspace cwd.
- starts session and runs a turn.
- extracts thread/turn/session IDs.
- emits normalized backend events.
- handles failures without stalling the orchestrator.
- keeps live tests credential/tool gated.

### 2b. Implement Full Claude Code Protocol Backend

Goal: evolve the conservative Claude Code subprocess path into the configured
Claude Code protocol flow while preserving the normalized backend interface.

Acceptance:

- launches configured Claude Code command in workspace cwd.
- starts a session or equivalent turn.
- emits normalized backend events.
- handles failures without stalling the orchestrator.
- keeps live tests credential/tool gated.

### 3. Harden Workspace Hooks And Cleanup

Goal: make workspace lifecycle credible for unattended runs.

Acceptance:

- hook timeout is enforced.
- `after_create`, `before_run`, `after_run`, and `before_remove` behavior match
  the parity roadmap.
- terminal cleanup can be planned from tracker reconciliation; automatic runtime
  cleanup remains pending.
- path escape tests cover symlink and non-directory cases.

### 4. Harden Workspace Branch And PR Handoff Reconciliation

Goal: strengthen the current live handoff path without mixing multiple issue
scopes in one branch.

Acceptance:

- reconciles existing worktrees with tracker state before reuse.
- records PR URL and branch evidence in the tracker workpad.
- detects missing remote commits or no-op branches before PR creation. (First
  dirty/no-op guard exists.)
- adds configured verification command execution before push/PR handoff. (First
  workflow-level command-list slice exists.)
- cleans terminal worktrees after tracker reconciliation.
- keeps main implementation completion at `Agent Review`.

### 5. Add Persistent Agent Review Worker Supervision

Goal: evolve the bounded `review loop` into persistent worker supervision.

Acceptance:

- reviewer backend is selected through config.
- one review worker per issue/PR is started and reconciled without batching
  unrelated issues into one prompt or blocking the main run-loop.
- review job identity, backend, artifact path, and result evidence are persisted
  in workpad/runtime state.
- duplicate review workers are prevented across repeated loop ticks.
- passed reviews move to `Human Review`; confirmed findings and completed but
  inconclusive automatic reviews move to `Rework`; failed, timed out, or
  unavailable reviews must not set `Human Review`.

### 5b. Preserve Review Freshness During Merging Rework

Goal: reduce repeated human review for mechanical Merging conflict repair
without weakening the review boundary.

Acceptance:

- records stale reason, rework class, prior/current head SHA, prior/current base
  SHA, changed files, and patch summary in workpad evidence.
- classifies mechanical conflict repair and base refresh as prior-review
  preserving only when evidence is explicit.
- classifies semantic or unknown rework as requiring the normal Agent Review and
  Human Review path.
- keeps `Human Review` out of the main implementation agent authority boundary.
- does not auto-approve or merge PRs.

### 6. Add Linear Credential-Gated Smoke Tests

Goal: prove the Linear adapter against a real workspace without making local
development depend on credentials.

Acceptance:

- skips cleanly when `LINEAR_API_KEY` or a configured project slug is absent.
- reads active project issues through the Linear adapter.
- updates one disposable issue state through mapped workflow state names.
- creates or updates a marker workpad comment.
- records exact schema gaps for any unsupported Linear mutation shape.

### 7. Expand Issue Forge Interaction Surface

Goal: turn the current CLI-first interactive and reflective flows into a richer
operator workflow only after the dry-run command-step path proves useful.

Acceptance:

- preserves the lightweight skill/template registry without product-specific
  roadmap assumptions.
- asks one focused clarification question at a time.
- keeps tracker creation behind explicit write and confirmation flags.
- can set tracker-neutral metadata fields such as Capability when the adapter
  supports them.
- remains fully testable in fixture mode.

## Dry-Run Dogfood Command

```bash
cargo run -- examples/dry-run-workflow.md
cargo run -- run-loop examples/dry-run-workflow.md --max-iterations 1 --dry-run
```

These commands should remain credential-free and deterministic. They are the
local smoke tests for dispatch planning and bounded loop behavior until live
GitHub Project v2 execution is hardened.

## Live Project Workflow

`workflows/jade-symphony.md` is the canonical non-fixture workflow for manual
live Project v2 reads and explicit tracker writes through `gh`. It uses the
local `tmux` backend for supervised lane execution, while prompt bodies remain
the real Jade Symphony dogfood operating contracts. `run-loop --write` records
attachable tmux session metadata, writes `session-registry.json` under the
configured artifact root, and keeps the issue active until real implementation
evidence is available for the existing handoff path. Manual recovery uses lane
claim commands first, then `session start --run <RUN_ID>` with the same registry
and lane prompt contracts. The registry is operator evidence for terminal
sessions only; it does not replace Project state.
