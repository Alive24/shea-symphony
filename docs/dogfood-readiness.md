# Dogfood Readiness

Status: dry-run dogfood baseline with read-only GitHub Project v2 loading through
the `gh` CLI and a `run-loop` skeleton that can idle-poll in unbounded write
mode. The live GitHub Project workflow now carries a real Jade operating prompt,
but Jade Symphony is not ready for unattended live GitHub Project v2 execution
yet.

## Current Capability Status

| Capability | Current Status |
| --- | --- |
| Workflow loader | Implemented for explicit workflow path and optional YAML front matter. Runtime reload is not implemented. |
| Dogfood workflow prompt | `examples/github-project-workflow.md` includes the Jade operating loop, issue quality gate expectation, workpad discipline, role boundaries, stop conditions, and one issue / one branch / one PR handoff rules. Tests guard against reverting to a placeholder-thin prompt. |
| Typed config | Implemented for the current skeleton, including GitHub Project v2-shaped settings. |
| Normalized issue model | Implemented as `TrackerIssue`, including ProjectV2 item ID, labels, assignees, blockers, linked PRs, and project fields. |
| GitHub Project v2 tracker | Fixture-backed mode plus live loading and explicit write operations through `gh api graphql`. `run-loop` can coordinate existing primitives, idle-poll in unbounded write mode, and use claim decision helpers before write-mode dispatch; auth diagnostics distinguish fixture mode, env-token auth, usable `gh api graphql` auth, missing `gh`, and unusable auth. Same-state status writes are skipped, Project item addition initializes the configured `Status` field to `Todo`, marker workpad upsert is idempotent for the canonical marker, and claim decision helpers distinguish claimable, active, and externally changed states. Full claim reconciliation is not implemented yet. |
| Linear tracker | Implemented behind the same trait for fixture-backed planning plus live GraphQL reads, state updates, marker workpad comments, follow-up issue creation, and project assignment. Credential-gated smoke coverage is still missing. |
| Issue Quality Gate | Implemented as a first-pass Markdown contract check. It is useful for dry-run classification, not yet a full source-alignment gate. |
| Issue Forge | Local CLI flows exist for discover, discuss, draft, validate, repair, CLI-first interactive issue shaping, conservative reflective follow-up candidate generation, and explicit `forge-create` tracker issue creation from quality-gated Markdown. Interactive creation requires `--write` and `--confirm-create`; reflective mode only prints candidates. Initial Project `Status` setup is available through the GitHub add-to-project path; arbitrary Project field setup after creation is not implemented yet. |
| Orchestrator | Deterministic dispatch planning and a CLI `run-loop` skeleton with bounded modes, idle polling, claim-helper use, runtime-state persistence, and planned handoff evidence exists. No long-running worker supervision, retry timers, full runtime resume reconciliation, or full state reconciliation yet. |
| Workspace | Local path sanitization, creation, timeout-aware hooks, stdout/stderr capture, `before_remove`, safe cleanup helpers, workspace/branch/PR handoff planning, and run-loop handoff evidence exist. Live git worktree creation, PR creation, and runtime reconciliation cleanup are not wired yet. |
| Agent backends | Dry-run backend plus conservative Codex and Claude Code subprocess backends exist. Full Codex app-server and Claude Code protocol parity are not implemented yet. |
| Agent Review | Finding classes, fake reviewer lifecycle, Gemini CLI subprocess backend, role-bound transition decisions, review-freshness evidence for Merging conflict repair, and workpad/status evidence helpers exist. Persistent review worker reconciliation is not implemented yet. |
| Observability | Operator-readable terminal snapshots report polling, running, retrying, skipped issues, gate details, token counters, event-log path, and integration gaps. JSONL event-log primitives exist and `run-once` writes dry-run events. Runtime state files are written during write-mode `run-loop` issue execution, but full resume reconciliation is still pending. No web/API surface yet. |
| Tests | Unit tests cover the dry-run skeleton. No credential-gated integration tests yet. |

## Before Executing GitHub Project Issues

These must exist before Jade Symphony can safely dogfood against real GitHub
Project v2 issues:

1. Harden read-only GitHub Project v2 adapter.
   - Keep loading ProjectV2 items through `gh api graphql` or replace it with a
     direct HTTP client behind the same adapter.
   - Filter to real GitHub Issues, not draft items or PR items.
   - Resolve configured status field and option IDs.
   - Normalize issue body, labels, assignees, linked PRs, project fields, and
     timestamps into `TrackerIssue`.
   - Add blocker mapping once the exact GitHub issue relationship source is
     selected.

2. Harden GitHub tracker writes.
   - Current explicit commands can set ProjectV2 status by option ID,
     create/update one marker workpad comment, create follow-up issues, and add
     issues to the configured project.
   - Mutating commands require `--write`.
   - Same-state status updates are treated as no-ops before mutation.
   - PR linking currently uses an issue comment/autolink strategy instead of a
     first-class relationship.
   - Remaining work: idempotency checks around project-item addition and richer
     reconciliation after writes.

3. Dispatch safety.
   - Enforce assignee filter from live GitHub issue assignees.
   - `run-loop` reuses tracker claim helpers to claim only `Todo` / `Rework`,
     resume active `In Progress`, and stop/replan on externally changed states.
   - Revalidate issue state immediately before dispatch.
   - Keep GitHub-specific fields out of `orchestrator`.
- Current explicit `gate-apply` can record quality-gate assumptions or
  missing context in the workpad and move non-dispatchable issues to
  `Need to Clarify` / `Need Human Input`; the future runtime must call that
  flow automatically before dispatch.

4. Agent Review authority boundary.
   - Main implementation commands can move locally complete work to
     `Agent Review` but cannot set `Human Review`.
   - Independent Review Agent commands can set `Human Review` only after a
     passed review with evidence.
   - Confirmed findings route to `Rework`.
   - Failed, timed out, inconclusive, or unavailable reviews remain out of
     `Human Review` and route to `Need Human Input` or stay in `Agent Review`.
   - Merging-to-Rework repairs must record review freshness evidence before
     preserving prior Human Review. Mechanical conflict repair can preserve
     prior Human Review for an authorized merge/handoff flow; semantic or
     unknown rework requires the normal Agent Review and Human Review path.

5. Linear integration hardening.
   - Add credential-gated live smoke tests for reads, state updates, and workpad
     upsert.
   - Confirm `commentUpdate` and project/team lookup against the active Linear
     workspace schema.
   - Keep Linear-specific GraphQL isolated to the adapter.
   - Preserve fixture mode for credential-free development.

6. Live agent execution.
   - The live dogfood workflow prompt now embeds enough Jade protocol for an
     isolated backend agent to understand the issue work cycle, workpad, review
     boundary, and stop conditions.
   - Keep the prompt aligned with `docs/bootstrap/JADE_WORKFLOW.md` as the
     bootstrap contract evolves.
   - Harden the Codex and Claude Code subprocess backends, then implement full
     protocol transports.
   - Keep Claude Code as a peer backend path.
   - Normalize session IDs, completion, failures, token usage, and rate-limit
     events.
   - Ensure external command execution happens only inside the prepared
     workspace.

7. Long-running orchestration.
   - Continuous idle polling exists for unbounded write mode; full active worker
     supervision is still pending.
   - claimed/running/retry state ownership.
   - Runtime state writes exist for claim/resume, backend result evidence, and
     final transition intent; full resume reconciliation after interruption
     remains pending.
   - workspace/branch/PR handoff planning is recorded by `run-loop`, but the
     runtime still needs live worktree creation, branch checkout, push, PR
     creation, and PR link recording.
   - continuation retry after normal active-state exits.
   - exponential backoff for failures.
   - stall detection.
   - terminal/non-active reconciliation.
- terminal workspace cleanup wiring in the runtime loop.

8. Observability.
   - Structured logs for dispatch, retry, state transition, backend session, and
     failure events.
   - Runtime snapshot command exists for dry-run and live-read planning; an API
     endpoint is still needed.
   - Clear integration-gap reporting when credentials are missing or unusable
     while avoiding false missing-token warnings when `gh api graphql` works.

9. Integration profile.
   - Credential-gated GitHub Project v2 smoke test.
   - Fixture tests for pagination, status option cache, malformed payloads, and
     permission failures.
   - Dry-run fixtures remain available for local development without credentials.

10. Issue Forge tracker completion.
   - `forge-create` can create a tracker issue and optionally add it to the
     configured project through the normalized adapter with initial `Todo`
     status in GitHub ProjectV2 mode.
   - `forge-interactive` can select a lightweight issue skill/template, ask a
     focused clarification question for thin intent, and print a quality-gated
     issue draft before any live tracker write.
   - `forge-reflect` can scan local context for conservative follow-up signals
     and print quality-gated draft candidates without creating live issues.
   - Remaining work: set capability and arbitrary Project fields through a
     tracker-neutral field operation or tracker-specific adapter method, plus a
     richer TUI or multi-step conversation surface if CLI steps become too thin.

## Recommended Next GitHub Issues

### 1. Harden Read-Only GitHub Project v2 Adapter

Goal: make the current `gh`-backed read adapter reliable enough for daily
planning, without adding writes yet.

Acceptance:

- loads ProjectV2 field metadata and configured Status field.
- lists only dispatchable GitHub Issue project items.
- normalizes issues into `TrackerIssue`.
- respects assignee filter and status mappings.
- has fixture tests for ProjectV2 payload shapes and a skipped live smoke test
  when no token is present.
- documents blocker-source limitations.

### 2. Harden GitHub Workpad And Status Writes

Goal: make the current explicit GitHub write commands safe enough for routine
operator use and ready for the future orchestrator loop.

Acceptance:

- updates ProjectV2 Status through option IDs.
- treats same-state status writes as no-ops.
- creates/reuses `<!-- jade-symphony-workpad -->` issue comments.
- records gate assumptions or missing context before dispatch or clarification.
- keeps write methods inside the GitHub adapter.
- has credential-gated smoke tests for status update and workpad upsert.
- add-project is idempotent or reports already-present items clearly.
- mutating CLI commands require explicit `--write`.

### 3. Turn Dispatch Plan Into Polling Runtime

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

### 4. Implement Full Codex App-Server Backend

Goal: evolve the conservative Codex subprocess path into full Codex app-server
protocol execution inside the workspace.

Acceptance:

- launches configured Codex command in workspace cwd.
- starts session and runs a turn.
- extracts thread/turn/session IDs.
- emits normalized backend events.
- handles failures without stalling the orchestrator.
- keeps live tests credential/tool gated.

### 4b. Implement Full Claude Code Protocol Backend

Goal: evolve the conservative Claude Code subprocess path into the configured
Claude Code protocol flow while preserving the normalized backend interface.

Acceptance:

- launches configured Claude Code command in workspace cwd.
- starts a session or equivalent turn.
- emits normalized backend events.
- handles failures without stalling the orchestrator.
- keeps live tests credential/tool gated.

### 5. Harden Workspace Hooks And Cleanup

Goal: make workspace lifecycle credible for unattended runs.

Acceptance:

- hook timeout is enforced.
- `after_create`, `before_run`, `after_run`, and `before_remove` behavior match
  the parity roadmap.
- terminal cleanup is tied to tracker reconciliation.
- path escape tests cover symlink and non-directory cases.

### 6. Wire Workspace Branch And PR Handoff Into Run-Loop

Goal: connect handoff planning evidence to controlled runtime mutation without
mixing multiple issue scopes in one branch.

Acceptance:

- records the planned workspace key, workspace path, branch, and PR title in
  `run-loop` dry-run output and workpad evidence.
- refuses branches that appear to belong to a different issue.
- creates or reuses one isolated git worktree per issue.
- checks out the planned issue branch from the configured base branch.
- pushes the issue branch after local completion.
- creates one PR with a handoff body and records the PR link in the workpad.
- keeps main implementation completion at `Agent Review`.

### 7. Add Agent Review Gate

Goal: make `Agent Review` a real state before `Human Review`.

Acceptance:

- reviewer backend is selected through config.
- findings are classified as `Confirmed`, `Plausible`, `Rejected`, or
  `Needs Context`.
- confirmed findings block human handoff.
- rejected/deferred findings are recorded in the workpad.
- main implementation agent completion target is `Agent Review`.
- independent Review Agent may move passed reviews to `Human Review` only after
  evidence is recorded.
- failed, timed out, inconclusive, or unavailable reviews must not set
  `Human Review`.

### 7b. Preserve Review Freshness During Merging Rework

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

### 8. Add Linear Credential-Gated Smoke Tests

Goal: prove the Linear adapter against a real workspace without making local
development depend on credentials.

Acceptance:

- skips cleanly when `LINEAR_API_KEY` or a configured project slug is absent.
- reads active project issues through the Linear adapter.
- updates one disposable issue state through mapped workflow state names.
- creates or updates a marker workpad comment.
- records exact schema gaps for any unsupported Linear mutation shape.

### 9. Expand Issue Forge Interaction Surface

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

## Live Project Template

`examples/github-project-workflow.md` is a non-fixture workflow template for
manual live Project v2 reads and explicit tracker writes through `gh`. It still
uses the `dry-run` backend by default, but the prompt body is now the real Jade
dogfood operating prompt rather than a placeholder. `run-loop --write` is
available only as a bounded runtime skeleton and should not be treated as full
autonomous agent execution until claim reconciliation, full runtime resume
reconciliation, and live PR automation wiring exist.
