# Jade Symphony

Jade Symphony is a Rust implementation of an OpenAI Symphony-style orchestration
harness with Jade-specific workflow extensions.

Current status: **dry-run skeleton, not a live orchestrator**.

The repo currently proves the core shape and has a small operator CLI: workflow
parsing, typed config, normalized tracker issues, fixture-backed and `gh`-backed
read-only GitHub Project v2 issue loading, dispatch planning, quality-gate
checks, backend abstractions, workspace safety helpers, event-log primitives,
and an operator-readable status snapshot with event-log and integration-gap
links.

It does **not** yet fully autonomously execute GitHub Project v2 issues,
run Codex/Claude through the final app-server flow, or supervise long-running
workers. A `run-loop` skeleton now exists and can idle-poll in unbounded
write mode, but full claim reconciliation, runtime resume, and
one-issue-one-PR automation are still future work.

## What Works Now

- Rust crate builds and tests locally.
- operator CLI parsing is backed by `clap` while preserving the existing safe
  command aliases.
- `WORKFLOW.md` style files load from an explicit path or default CLI path.
- YAML front matter is parsed into typed runtime config.
- normalized `TrackerIssue` records can be loaded from JSON fixtures.
- GitHub Project v2 read-only issue loading can use the `gh` CLI when no fixture
  path is configured.
- GitHub Project v2 integration-gap reporting distinguishes fixture mode,
  env-token auth, usable `gh api graphql` auth, missing `gh`, and unusable auth.
- Linear tracker reads and explicit write operations are implemented behind the
  same adapter trait; fixture mode remains the credential-free path.
- explicit live GitHub write commands exist for ProjectV2 status updates,
  workpad comments, follow-up issue creation, and adding issues to a project when
  not in fixture mode.
- GitHub Project v2 status writes skip same-state mutations, and tracker claim
  helpers distinguish claimable `Todo`/`Rework`, already active
  `In Progress`, and externally changed states.
- dry-run dispatch planning sorts by priority and respects global/state
  concurrency limits.
- Issue Quality Gate classifies executable versus underspecified issue bodies.
- Issue Forge can discover local candidates from intent, ask one focused
  clarification question, draft from the quality template, validate Markdown,
  repair rough Markdown into an executable issue contract shape, and create a
  tracker issue from a quality-gated contract with explicit `--write`.
- basic strict prompt rendering supports known `issue.*` fields, `attempt`, and
  simple `{% if %}` / `{% else %}` blocks.
- workspace identifiers are sanitized; local workspace paths stay under the
  configured root; hooks support timeouts, stdout/stderr capture,
  `before_remove`, and safe cleanup helpers.
- workspace/branch/PR handoff planning can derive a deterministic issue
  workspace key, branch name, and PR handoff body, and can detect an existing
  branch that appears to belong to a different issue; profile-scoped workspace
  keys can avoid collisions between parallel worker identities.
- execution profiles can be listed from workflow config. The first slice can
  read cockpit-tools Codex instance stores (`codex_instances.json`) and treats
  each instance name as a Jade worker profile without reading or logging account
  bindings.
- terminal status output reports polling state, planned running/skipped/retrying
  issues, token counters, event-log path, gate details, and integration gaps.
- JSONL event-log primitives exist and can record selected profile identity.
- runtime state helpers can write, read, and clear a tracker-neutral
  `runtime/runtime-state.json` file under the configured logs root, including
  optional profile and instance identity.
- write-mode `run-loop` saves active issue runtime state, updates it with
  backend result evidence, records final transition intent, and clears it after
  successful handoff/block transition.
- `run-once` can prepare one dry-run workspace, render a prompt file, run the
  dry-run backend, and append JSONL events.
- `run-once` can execute the conservative Codex subprocess backend when a
  workflow explicitly sets `agent.backend: codex`.
- `run-once` can execute the conservative Claude Code subprocess backend when a
  workflow explicitly sets `agent.backend: claude-code`.
- `run-loop` can re-read tracker state per iteration, select dispatchable work,
  print dry-run claim/run/workpad/handoff actions, surface deterministic
  workspace/branch/PR handoff plans, use tracker claim helpers to
  claim/resume/skip externally changed issues, and in explicit `--write` mode
  run one issue at a time, record planned handoff evidence, and stop main-agent
  completion at `Agent Review`;
  unbounded write mode sleeps on idle polls using the workflow polling interval.

## Dry-Run Only

- `github_project_v2` adapter is dry-run when `tracker.fixture_path` is set.
- `memory` tracker reads fixture issues only.
- `linear` adapter is dry-run when `tracker.fixture_path` is set.
- CLI dispatch is a plan/status snapshot, not worker execution.
- `dry-run` backend emits normalized fake events for tests only.

Run the bundled dry-run example:

```bash
cargo run -- examples/dry-run-workflow.md
```

Expected shape:

- ready fixture issues appear under `running issues`.
- underspecified, blocked, terminal, or over-limit fixture issues appear under
  `skipped issues`.
- `event_log=...` points at the configured JSONL event stream.
- GitHub Project v2 integration gaps are printed honestly.

## Operator Commands

```bash
cargo run -- validate examples/dry-run-workflow.md
cargo run -- validate-workflow examples/dry-run-workflow.md
cargo run -- inspect examples/dry-run-workflow.md
cargo run -- plan examples/dry-run-workflow.md
cargo run -- plan-dispatch examples/dry-run-workflow.md
cargo run -- status examples/dry-run-workflow.md
cargo run -- run-once examples/dry-run-workflow.md
cargo run -- run-once examples/codex-subprocess-workflow.md
cargo run -- run-once examples/claude-subprocess-workflow.md
cargo run -- run-loop examples/dry-run-workflow.md --max-iterations 1 --dry-run
cargo run -- profiles examples/cockpit-profiles-workflow.md
cargo run -- plan examples/linear-fixture-workflow.md
cargo run -- gate examples/dry-run-workflow.md '#3'
cargo run -- forge-discover --intent "Add Issue Forge validate and repair commands"
cargo run -- forge-validate --title "Thin Forge issue" --file examples/fixtures/thin-issue.md
cargo run -- forge-discuss --title "Thin Forge issue" --file examples/fixtures/thin-issue.md
cargo run -- forge-repair --title "Thin Forge issue" --file examples/fixtures/thin-issue.md
cargo run -- forge-validate --title "Repaired Forge issue" --file examples/fixtures/repaired-issue.md
cargo run -- forge-draft --title "Implement read-only Project v2 adapter" --goal "Load Project v2 issues into TrackerIssue records."
cargo run -- forge-create --workflow path/to/WORKFLOW.md --title "Follow-up title" --file path/to/issue.md --add-to-project --write
```

`run-once` defaults to dry-run examples, but the Codex and Claude subprocess
fixtures show controlled real-backend paths without invoking live hosted
services. They write `JADE_SYMPHONY_PROMPT.md` into the prepared workspace and
append JSONL events for the selected workflow.

`profiles` lists execution profiles discovered from workflow config. For
cockpit-tools, Jade currently reads the Codex instance store shape inspected in
`https://github.com/jlcodes99/cockpit-tools`: a local `codex_instances.json` with camelCase
`instances[]` records such as `name`, `userDataDir`, `workingDir`, and
`extraArgs`. Jade ignores account binding fields, uses the instance name as the
worker identity, and falls back to explicit `profiles.entries` when the
cockpit-tools file is not present. This is a small adapter boundary, not a full
account manager.

Live GitHub write commands are explicit and require a non-fixture workflow plus
usable GitHub auth through `GITHUB_TOKEN` / `GH_TOKEN` or `gh api graphql`:

```bash
cargo run -- set-state path/to/WORKFLOW.md '#123' need_to_clarify --write
cargo run -- workpad path/to/WORKFLOW.md '#123' path/to/workpad.md --write
cargo run -- create-follow-up --workflow path/to/WORKFLOW.md --title "Follow-up title" --body-file path/to/body.md --write
cargo run -- add-to-project path/to/WORKFLOW.md <github-issue-node-id> --write
cargo run -- gate-apply path/to/WORKFLOW.md '#123' --write
cargo run -- review-once path/to/WORKFLOW.md '#123' --write
cargo run -- review-fake path/to/WORKFLOW.md '#123' --outcome pass --write
```

`add-to-project` initializes the configured ProjectV2 `Status` field to the
workflow's mapped `Todo` option so newly added issues are visible to the
normalized tracker state machine. Arbitrary Project field setup, such as
Capability, is still a follow-up.

`set-state` is a main-implementation-agent command and refuses `Human Review`.
`review-once` / `review-fake` are independent Review Agent commands: a passing
review can move `Agent Review` to `Human Review`, confirmed findings move to
`Rework`, and failed or inconclusive reviews do not advance to `Human Review`.
These commands are adapter operations plus the first runtime-loop
skeleton, not full autonomous orchestration. Use write mode carefully until
claim reconciliation, resume state, and PR automation exist.

## Stubbed

- linked PR attachment/linking as a first-class relationship.
- live git worktree creation and `gh pr create` from the runtime loop.
- full multi-account manager UI or account switching; cockpit-tools integration
  is currently read-only fixture/path parsing for Codex instance names.
- Linear live adapter credential-gated smoke coverage.
- Codex app-server transport.
- Claude Code full protocol transport beyond the subprocess fixture path.
- dynamic tool registry such as `linear_graphql`.
- runtime workflow reload and long-running worker supervision.
- web/API observability surface.

## Not Implemented Yet

- long-running worker supervision beyond idle polling in `run-loop`.
- richer issue claiming, state transition, and reconciliation safety beyond the
  current claim helper and runtime-state wiring.
- full runtime-state resume reconciliation after interruption.
- workspace-per-issue branch and PR automation beyond current handoff planning
  and workpad evidence.
- retry timers, continuation retries, and stall restart.
- terminal workspace cleanup tied to tracker state.
- profile-aware tracker claim ownership beyond namespaced runtime/log/workspace
  metadata.
- live token/rate-limit accounting beyond the current snapshot counters.
- persistent Agent Review worker supervision and reconciliation.
- Issue Forge Project field setup after issue creation.
- automated Issue Quality Gate application inside the polling runtime.
- full Liquid-compatible prompt renderer.
- credential-gated integration tests.

## Read-Only GitHub Project v2 Use

Remove `tracker.fixture_path` from a workflow and make sure `gh` is installed and
authenticated:

```bash
gh auth status
cargo run -- inspect path/to/WORKFLOW.md
cargo run -- plan path/to/WORKFLOW.md
```

This path can read ProjectV2 items and normalize GitHub Issue content for
planning. Explicit `--write` commands can update ProjectV2 status, write workpad
comments, create follow-up issues, and add issues to the project with initial
`Todo` status. PR linking uses an issue comment/autolink strategy rather than a
first-class relationship. Jade
Symphony can idle-poll in unbounded write mode, but still cannot fully reconcile
state or supervise live agents.

## Development Commands

```bash
cargo test
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

Dry-run dispatch:

```bash
cargo run -- plan examples/dry-run-workflow.md
cargo run -- run-once examples/dry-run-workflow.md
cargo run -- run-loop examples/dry-run-workflow.md --max-iterations 1 --dry-run
```

The dry-run workflow uses:

- `examples/dry-run-workflow.md`
- `examples/fixtures/dry-run-issues.json`
- `examples/linear-fixture-workflow.md`
- `examples/fixtures/linear-issues.json`

For a real GitHub Project v2 read/write workflow template, copy and edit:

- `examples/github-project-workflow.md`

Update `owner`, `repo`, `project_owner`, `project_number`, and `state_map` before
using it with a live project.

## Bootstrap Sources

The implementation is grounded in `docs/bootstrap/` and the pinned official
reference under `docs/bootstrap/references/openai-symphony`.

Do not edit files under `docs/bootstrap/references/openai-symphony`.
