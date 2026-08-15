# Shea Symphony Skill Suite

Release: `2026.08.15`

This directory is the single auditable source for Shea Symphony's installable
skills. The standard open Skills CLI installs project-local copies or links for
Codex, Claude Code, Antigravity, and other compatible harnesses; Shea does not
maintain a second installer or its own harness-path table.

## Install `setup-shea` First

From a target repository, install the one conversational entry skill without a
Shea checkout:

```bash
npx skills add https://github.com/Alive24/shea-symphony/tree/main/skills/shea-symphony/suite/setup-shea
```

Project scope is the Skills CLI default. It detects available harnesses and asks
which project-local surfaces to configure. Do not add `-g` for repository setup.
Invoke `setup-shea` after installation; it keeps the operator in one
conversation while it discovers the target, previews a complete plan, obtains
confirmation, invokes the standard Skills CLI for the selected normal set,
installs or reuses a verified Legacy runtime, reconciles repository-owned
contracts, and performs no-claim readiness.

For source-suite inspection, list the exact pinned revision before installing:

```bash
npx skills add https://github.com/Alive24/shea-symphony/tree/<revision>/skills/shea-symphony/suite --list
```

After the `setup-shea` plan is visible and explicitly confirmed, its standard
non-interactive install shape is:

```bash
npx skills add https://github.com/Alive24/shea-symphony/tree/<revision>/skills/shea-symphony/suite \
  --skill setup-shea \
  --skill shea-symphony-runtime-onboarding \
  --skill shea-symphony-doctor \
  --skill shea-symphony-issue-forge \
  --skill shea-symphony-investigate \
  --skill shea-symphony-issue-forge-reflect \
  --skill shea-symphony-manual-main \
  --skill shea-symphony-manual-review \
  --skill shea-symphony-human-review \
  --skill shea-symphony-manual-merge \
  --agent codex \
  --agent claude-code \
  --agent antigravity \
  -y
```

Only selected, available agents are included. The standard CLI owns update and
removal as well:

```bash
npx skills list
npx skills update -p -y
npx skills remove <skill> --agent <agent> -y
```

Before installing or starting a skill-dependent session, inspect readiness
without writing local skill roots:

```bash
<resolved-shea-symphony-legacy> skills status .shea/workflows/shea-symphony.md
<resolved-shea-symphony-legacy> skills status .shea/workflows/shea-symphony.md --json
<resolved-shea-symphony-legacy> skills status .shea/workflows/shea-symphony.md --session-skills "shea-symphony-manual-main,shea-symphony-doctor"
```

`skills status` treats this suite as the expected source, then compares the
configured project-local install, rendered metadata, symlink or alias shape, and
optional current-session skill visibility. Source suite discovery is
`--suite-path`, `SHEA_SYMPHONY_SKILL_SUITE`, current repo
`skills/shea-symphony/suite`, then installed-only mode. Missing session input is
reported as `unknown`, not as a failure. `setup-shea` also reads the standard
project paths chosen by the Skills CLI: `.agents/skills` for Codex and
Antigravity, and `.claude/skills` for Claude Code.

## Normal Skill Set

- `setup-shea`
- `shea-symphony-issue-forge`
- `shea-symphony-investigate`
- `shea-symphony-issue-forge-reflect`
- `shea-symphony-manual-main`
- `shea-symphony-runtime-onboarding`
- `shea-symphony-manual-review`
- `shea-symphony-human-review`
- `shea-symphony-manual-merge`
- `shea-symphony-doctor`

`shea-symphony-issue-forge-dream` and HALO research are explicit additions,
not normal setup. Dream is marked internal and the controller always names the
normal skills explicitly, so research skills are never added implicitly.

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

The Doctor skill is a read-first operator triage slot. Its
`repository_contract_repair` path diagnoses repository-owned workflow, prompt,
workpad-template, and skill contracts, previews the smallest safe diff, and
applies only an explicitly confirmed path-bounded edit. Automatic rewriting,
tracker mutation, global skill changes, and CLI-owned runtime-envelope edits
remain out of scope. `setup-shea` uses the normal-skill manifest and the
standard Skills CLI rather than maintaining a separate source-copy validator.

Runtime Onboarding inspects repository-owned requirement evidence and existing
installed tools, reports conflicts, and prepares a credential-free
`.shea/runtime-profile.json` proposal. It requires operator confirmation before
writing the machine-local profile and never installs tools or edits shell or
system configuration.

## Dogfood Entry Points

For normal all-lane dogfood, operators should run the CLI directly instead of
starting three independent manual skills:

```bash
cargo run -- autopilot plan .shea/workflows/shea-symphony.md
cargo run -- autopilot loop .shea/workflows/shea-symphony.md --max-iterations 1 --write
```

`autopilot plan` is read-only. `autopilot loop` is a bounded foreground
supervisor, not a daemon, background service, or app-server. Use Manual Main,
Manual Review, and Manual Merge only for focused debugging, break-glass recovery,
or operator-selected lane-specific work after the normal Autoloop preflight
points at that lane.
