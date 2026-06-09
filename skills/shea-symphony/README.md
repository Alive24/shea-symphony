# Shea Symphony Skill Suite

Release: `2026.06.09`

This directory contains the repo-owned Shea Symphony skills used by local Codex
and Gemini operator sessions. The suite is intentionally versioned in the repo
so skill behavior can be reviewed with workflow docs, prompts, and CLI changes.

## Install Or Preview

Preview the detected local targets without writing:

```bash
node scripts/install-shea-symphony-skills.js --dry-run
```

Install or update after an interactive confirmation:

```bash
node scripts/install-shea-symphony-skills.js
```

Install non-interactively only after choosing explicit targets:

```bash
node scripts/install-shea-symphony-skills.js \
  --codex-dir "$HOME/.codex/skills" \
  --gemini-dir "$HOME/.gemini/local-skills" \
  --yes
```

Validate active local copies against the repo-owned suite:

```bash
node scripts/install-shea-symphony-skills.js --validate
```

Before installing or starting a skill-dependent session, inspect readiness
without writing local skill roots:

```bash
cargo run -- skills status workflows/shea-symphony.md
cargo run -- skills status workflows/shea-symphony.md --json
cargo run -- skills status workflows/shea-symphony.md --session-skills "shea-symphony-manual-main,shea-symphony-doctor"
```

`skills status` treats this suite as the expected source, then compares Codex
and Gemini local installs, rendered metadata, symlink or alias shape, and
optional current-session skill visibility. Source suite discovery is
`--suite-path`, `SHEA_SYMPHONY_SKILL_SUITE`, current repo
`skills/shea-symphony/suite`, then installed-only mode. Missing session input is
reported as `unknown`, not as a failure. Gemini is optional unless the operator
passes `--require-gemini` or otherwise configures a Gemini skill root.

The installer detects:

- Codex target from `CODEX_HOME/skills`, then `$HOME/.codex/skills`.
- Gemini target from `GEMINI_HOME/local-skills`, then `$HOME/.gemini/local-skills`.

Use `--skip-codex`, `--skip-gemini`, `--codex-dir`, or `--gemini-dir` to make
the target set explicit. Normal install mode shows every target and requires
operator confirmation before writing.

## Packaged Skills

- `shea-symphony-issue-forge`
- `shea-symphony-investigate`
- `shea-symphony-issue-forge-reflect`
- `shea-symphony-issue-forge-dream`
- `shea-symphony-manual-main`
- `shea-symphony-manual-review`
- `shea-symphony-human-review`
- `shea-symphony-manual-merge`
- `shea-symphony-doctor`

Investigate is the pre-Forge exploration slot. It gathers read-only evidence,
keeps competing hypotheses visible, classifies ambiguous Shea Symphony symptoms
or ideas, and recommends whether to continue investigating, hand off to Issue
Forge, use Doctor or Human Review, record a backlog seed, or take no action.

Human Review briefs the operator after Review Agent pass evidence, guides
operator-owned UAT, records a structured decision note, and routes only after
explicit confirmation. Accepted Human Review goes to `Merging`, not `Done`.
Routine native subissues should not invoke Human Review directly; passing
subissue Agent Review routes to `Merging` unless the child records
`Subissue Human Review Exception: <reason>`.

Dream is the slow, deep backlog mining skill. It writes bounded advisory Dream
Logs under `docs/dream-log/`, updates the compact Dream index, and creates
enriched `Backlog` seeds by default unless the operator asks for report-only
mode. It never creates `Todo` issues directly.

The Doctor skill is a read-first operator triage slot. The Shea Symphony CLI
`doctor` command reports local install-health warnings, while automatic repair
remains out of scope and install/update writes stay behind the confirmed #242
installer path.

## Dogfood Entry Points

For normal all-lane dogfood, operators should run the CLI directly instead of
starting three independent manual skills:

```bash
cargo run -- autopilot plan workflows/shea-symphony.md
cargo run -- autopilot loop workflows/shea-symphony.md --max-iterations 1 --write
```

`autopilot plan` is read-only. `autopilot loop` is a bounded foreground
supervisor, not a daemon, background service, or app-server. Use Manual Main,
Manual Review, and Manual Merge only for focused debugging, break-glass recovery,
or operator-selected lane-specific work after the normal Autoloop preflight
points at that lane.
