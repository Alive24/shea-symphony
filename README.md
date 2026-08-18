# Shea Symphony

Shea Symphony is a local orchestration system and desktop operator cockpit for
running AI-assisted engineering work through explicit implementation, review,
human approval, merge, and recovery boundaries.

## Operator cockpit

The Shea Symphony App keeps lane queues, human decisions, local worktrees, and
issue-level evidence visible in one foreground workspace.

![Shea Symphony Operator Desk](docs/assets/screenshots/operator-desk.png)

Shea Symphony helps an operator:

- shape rough intent into tracker-backed, executable issue contracts;
- move implementation through separate Main, Agent Review, Human Review, and
  Merge boundaries; and
- understand and recover interrupted work from durable evidence instead of
  reconstructing an agent session from scratch.

<details>
<summary>More App views</summary>

The Lanes view brings the current human queue, active lane work, and local
issue worktrees together.

![Shea Symphony Lanes overview](docs/assets/screenshots/lanes-overview.png)

The issue view connects tracker state with worktree provenance, handoff links,
and lifecycle evidence.

![Shea Symphony issue view](docs/assets/screenshots/lane-issue-view.png)

</details>

## Public entrypoints

- Use the global [`setup-shea`](.agents/skills/setup-shea/SKILL.md) Skill to
  onboard or reconcile a repository. It installs an immutable release-selected
  contract while leaving the resulting repository resources customizable.
  `setup-shea` itself is not vendored into the target repository.
- Use the [Shea Symphony App](app/README.md) as the normal operator surface.
  The Legacy CLI is an internal compatibility adapter used by the App, Skills,
  and bounded recovery paths; it is not the normal user interface.

## Documentation

- [OpenWiki](openwiki/index.md) is the public documentation and navigation
  surface. Its [freshness metadata](openwiki/.last-update.json) must be checked
  before treating a generated page as current.
- [`docs/README.md`](docs/README.md) routes coding agents to the smallest
  authoritative repository context for a task.

OpenWiki is derived. Source code, tests, workflow configuration, repository
contracts, accepted decisions, and the live tracker remain authoritative for
their respective facts.

## Runtime transition

The protected 2606 MVP remains a behavior and recovery baseline while 2607
replaces its hand-rolled orchestration with Temporal. On current `main`, the
default `shea-symphony` executable is the Temporal worker. The App temporarily
uses the separately identified `shea-symphony-legacy` sidecar for compatibility
operations; see [`docs/legacy-runtime-distribution.md`](docs/legacy-runtime-distribution.md).

The active design and dated implementation snapshot live under
[`docs/milestones/2607-hardening/`](docs/milestones/2607-hardening/).

## Development checks

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
npm --prefix app test
npm --prefix app run check
```
