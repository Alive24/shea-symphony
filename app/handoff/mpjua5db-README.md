# Shea Symphony

Shea Symphony is a team workflow system for supervised AI-native engineering.

It helps a human operator turn rough engineering intent into issue contracts,
run implementation agents in isolated workspaces, request independent agent
review, preserve audit evidence, and land approved pull requests through a
guarded merge lane.

It is inspired by OpenAI Symphony, but the focus here is not just launching an
agent. The focus is the whole team loop around the agent:

- what work is safe to start;
- who or what currently owns it;
- where the implementation happened;
- which evidence proves it is ready;
- when a human must decide;
- how the merge should be repaired, retried, or stopped.

Current maturity: **supervised team-workflow dogfood**. Shea Symphony can run
bounded Main, Review, and Merge lane ticks against a live tracker. It is moving
toward all-lane autopilot, but write-mode automation is still deliberately
observable, bounded, and operator-led.

## The Short Version

Modern coding agents are good at making changes. Teams need more than that.

A real team needs a way to say:

- this issue is clear enough to dispatch;
- this agent is allowed to work on it;
- this work happened in the right branch and worktree;
- this PR was independently reviewed;
- this human approval was recorded;
- this merge failure is mechanical, semantic, or blocked;
- this run can be resumed without guessing.

Shea Symphony turns those questions into a workflow.

```mermaid
flowchart LR
    A["Rough idea"] --> B["Issue Forge"]
    B --> C["Todo issue contract"]
    C --> D["Main lane"]
    D --> E["Agent Review"]
    E --> F["Human Review"]
    F --> G["Merge lane"]
    G --> H["Done"]

    E --> I["Rework"]
    I --> D
    G --> J["Need Human Input"]
    J --> F
```

The tracker stays the shared source of truth. Local artifacts, worktrees, logs,
and session records exist to make the tracker state explainable and recoverable,
not to replace it.

## How People Use It

Shea Symphony is designed around a human operator, not a hidden daemon.

The operator can ask:

1. What is ready to work on?
2. What is blocked or ambiguous?
3. Which lane should run next?
4. Did the agent leave enough evidence?
5. Is this safe to approve, repair, or merge?

The system answers through a few surfaces:

- **Issue Forge** shapes rough work into executable issues.
- **Main lane** implements one issue in an isolated workspace and stops at
  `Agent Review`.
- **Review lane** performs independent agent review and records pass or rework
  evidence.
- **Human Review** gives the operator a structured approval checkpoint.
- **Merge lane** lands approved PRs, repairs safe mechanical drift, and routes
  real uncertainty to `Need Human Input`.
- **Workpads and timeline evidence** keep the issue readable after the run.
- **Doctor and status surfaces** explain stuck states without requiring a
  low-level log expedition.
- **Dream and Reflect skills** mine recent work into safe Backlog candidates
  before they become executable Todo issues.

The intended feeling is closer to a team cockpit than a prompt runner. You
should be able to leave work moving, come back later, and understand what
happened from the issue, PR, workpad, and status output.

## A Human-First Tour

If you are evaluating Shea Symphony, start with these questions.

### 1. Do you have a tracker-backed workflow?

Shea Symphony expects real work to live in a tracker. The current self-dogfood
workflow uses GitHub Issues plus GitHub Project v2. Linear support exists behind
the same tracker abstraction, but the strongest dogfood path today is GitHub.

The workflow file describes tracker states, lane prompts, runtime configuration,
artifact roots, and verification expectations:

```bash
cargo run -- validate workflows/shea-symphony.md
cargo run -- project state workflows/shea-symphony.md
```

### 2. Is the issue ready for an agent?

Agents should not start from vibes alone. Issue Forge checks whether an issue
has the contract shape needed for safe execution: goal, context, guardrails,
scope, dependencies, verification, and expected outcome.

In normal use, the conversational part happens through the Shea Symphony Codex
skills. The CLI stays deterministic and scriptable:

```bash
cargo run -- forge validate \
  --workflow workflows/shea-symphony.md \
  --status Todo \
  --title "<title>" \
  --body-file /path/to/issue.md
```

Backlog seeds can stay intentionally softer. Todo issues are dispatchable and
must pass the stronger gate.

### 3. What would the system do next?

Before writing anything, ask the system for a read-only plan:

```bash
cargo run -- autopilot plan workflows/shea-symphony.md
cargo run -- main loop workflows/shea-symphony.md --max-iterations 1 --dry-run
cargo run -- review loop workflows/shea-symphony.md --max-iterations 1 --dry-run
cargo run -- merge loop workflows/shea-symphony.md --max-iterations 1 --dry-run
```

`autopilot plan` is the bridge toward all-lane automation. It does not launch
workers. It shows lane readiness, parked human queues, runtime concerns,
doctor findings, and the next likely actions.

### 4. When should you allow writes?

Write mode is explicit because tracker mutation is real team state.

For a bounded supervised tick:

```bash
cargo run -- main loop workflows/shea-symphony.md --max-iterations 1 --write
cargo run -- review loop workflows/shea-symphony.md --max-iterations 1 --write
cargo run -- merge loop workflows/shea-symphony.md --max-iterations 1 --write
```

The canonical operator launcher wraps the same idea:

```bash
scripts/shea-dogfood --dry-run
scripts/shea-dogfood --write --confirm-write --max-iterations 1
```

The system is intentionally conservative. It should prefer a visible blocked
state over a silent unsafe advance.

## The Lane Model

Shea Symphony separates work by authority.

### Main Lane

The Main lane is for implementation. It claims a Todo or Rework issue, prepares
or resumes an isolated worktree, runs the configured agent backend, verifies the
change, opens or reuses a PR, records the Main Workpad, and stops at
`Agent Review`.

The Main agent must not approve its own work.

The canonical workflow now defaults Main execution to Codex app-server. tmux
remains available as an explicit fallback/debug substrate, but it is no longer
the primary unattended direction.

### Review Lane

The Review lane is independent review. In the current dogfood path, automatic
review uses headless Gemini CLI and records a durable review job ledger plus a
human-readable issue comment.

Passing review can route ordinary issues to `Human Review`. Routine native
subissues can route directly to `Merging` when the parent issue owns final UAT.
Confirmed findings route to `Rework`.

### Human Review

Human Review is not ceremonial. It is the place where the operator checks the
issue, PR, review evidence, and UAT expectations before approving the work for
merge.

Human Review is intentionally skill-guided: the operator should get a briefing,
understand what changed, run or inspect the right checks, and then make an
explicit decision.

### Merge Lane

The Merge lane owns landing approved work. Clean merges stay direct CLI
behavior; they do not need an LLM. Behind PRs can be updated and retried.
Mechanical conflict repair should stay inside the merge lane when safe.

`Need Human Input` is reserved for semantic uncertainty, unsafe state,
verification failure, missing evidence, or infrastructure failures that need an
operator decision.

Merge repair should not erase the fact that the issue already passed Agent
Review and Human Review. It should preserve reviewed intent, record what
changed, and land only when the result is still safe.

## Evidence Surfaces

Shea Symphony is opinionated about evidence because agent work without evidence
turns into archaeology.

- The **issue body** is the contract.
- The **Main Workpad** is the current implementation surface.
- **Timeline comments** record Review, Human Review, Merge, Rework, and Doctor
  events.
- The **PR** is the code handoff and must be visible through linked-PR readback.
- **Local artifacts** store prompts, app-server protocol output, stderr,
  normalized events, review ledgers, and session registry records.
- **Doctor** connects tracker state, runtime state, and local evidence into a
  readable diagnosis.

The goal is not to keep every byte forever. The goal is that a human can answer
"what happened here?" without guessing.

## Relationship To OpenAI Symphony

Shea Symphony follows the OpenAI Symphony lineage for workflow loading, agent
execution, app-server direction, tracker-backed operation, runtime state, and
operator surfaces.

It extends that lineage for a more explicit team workflow:

- Issue Forge and quality gates before dispatch;
- GitHub Project v2 and Linear tracker state machines;
- separate Main, Review, Human Review, and Merge authority boundaries;
- workpad and timeline evidence conventions;
- parent/subissue branch topology;
- Doctor diagnostics for stuck states;
- repo-owned skills for conversational operator workflows;
- Reflect and Dream loops for safe backlog formation;
- read-only autopilot planning before write-mode all-lane automation.

Pinned upstream references live under
[`docs/bootstrap/references/openai-symphony`](docs/bootstrap/references/openai-symphony/).
Do not edit those reference files.

## What Works Today

The current self-dogfood workflow can:

- load and validate workflow files;
- read GitHub Project v2 tracker state;
- validate issue contracts before dispatch;
- create and promote tracker issues through Forge;
- run bounded Main, Review, and Merge lane ticks;
- use Codex app-server for Main execution;
- use headless Gemini for Review execution;
- create isolated issue worktrees and PR handoffs;
- preserve Main workpads and lane timeline evidence;
- recover interrupted Main and Merge lane work by default;
- inspect runtime/session status;
- diagnose tracker, PR, worktree, skill, runtime, and lane-state problems;
- plan future all-lane autopilot actions without mutating state.

It is still not a hosted production orchestrator. Long-running all-lane
autopilot, richer app-server observation, broader hosted dashboards, full remote
worker supervision, and deeper cross-provider policy controls are active
follow-up areas.

## Operator Quickstart

Build and verify locally:

```bash
cargo build
cargo test
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

Inspect the canonical self-dogfood workflow:

```bash
cargo run -- validate workflows/shea-symphony.md
cargo run -- project state workflows/shea-symphony.md
cargo run -- doctor workflows/shea-symphony.md
cargo run -- debug workflows/shea-symphony.md
```

Preview the next lane actions:

```bash
cargo run -- autopilot plan workflows/shea-symphony.md
cargo run -- main loop workflows/shea-symphony.md --max-iterations 1 --dry-run
cargo run -- review loop workflows/shea-symphony.md --max-iterations 1 --dry-run
cargo run -- merge loop workflows/shea-symphony.md --max-iterations 1 --dry-run
```

Run a bounded write tick only when the preview and Doctor output make sense:

```bash
cargo run -- main loop workflows/shea-symphony.md --max-iterations 1 --write
```

For the full operator runbook, read
[`docs/operator-dogfood.md`](docs/operator-dogfood.md). For command details,
read [`docs/cli-command-reference.md`](docs/cli-command-reference.md).

## Project Map

- [`workflows/shea-symphony.md`](workflows/shea-symphony.md): canonical
  self-dogfood workflow.
- [`workflows/prompts/`](workflows/prompts/): Main, Review, and Merge lane
  prompt contracts.
- [`skills/shea-symphony/`](skills/shea-symphony/): installable Shea Symphony
  skills for Codex and Gemini operator sessions.
- [`docs/operator-dogfood.md`](docs/operator-dogfood.md): supervised operator
  launcher and live-run guidance.
- [`docs/cli-command-reference.md`](docs/cli-command-reference.md): command
  behavior, write boundaries, and examples.
- [`docs/dogfood-readiness.md`](docs/dogfood-readiness.md): detailed capability
  inventory and known gaps.
- [`docs/bootstrap-parity-audit.md`](docs/bootstrap-parity-audit.md): OpenAI
  Symphony parity and extension audit.
- [`docs/parent-subissue-topology.md`](docs/parent-subissue-topology.md):
  parent/subissue branch and review semantics.
- [`docs/artifact-storage-policy.md`](docs/artifact-storage-policy.md):
  artifact durability and cleanup policy.
- [`docs/bootstrap/`](docs/bootstrap/): extension notes and pinned upstream
  references.
- [`examples/`](examples/): fixture workflows and safe local examples.

## Design Boundaries

Shea Symphony is orchestration infrastructure. It should not contain downstream
application business logic. Domain work belongs in tracked issues and isolated
issue workspaces.

The tracker is the operating source of truth. Local runtime files make tracker
state recoverable and auditable, but lane decisions should refresh live tracker
state before claiming, reviewing, or merging work.

Role boundaries matter:

- Main implementation stops at `Agent Review`.
- Review evidence gates movement toward `Human Review` or `Merging`.
- Human approval gates ordinary merges.
- Merge repair stays in the merge lane unless it needs a real human decision.
- Dream and Reflect output is advisory until promoted into an issue contract.

Write-mode commands should record evidence before state transitions, preserve
claims and audit records, and fail closed when the safe next action is unclear.

## Development

The main verification commands are:

```bash
cargo test
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

Useful read-only commands:

```bash
cargo run -- validate examples/dry-run-workflow.md
cargo run -- project inspect examples/dry-run-workflow.md '#1'
cargo run -- plan examples/dry-run-workflow.md
cargo run -- status show examples/dry-run-workflow.md --json
cargo run -- clean plan workflows/shea-symphony.md
```

The implementation is grounded in `docs/bootstrap/` and the pinned official
reference under `docs/bootstrap/references/openai-symphony/`.
