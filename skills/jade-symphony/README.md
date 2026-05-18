# Jade Symphony Skill Suite

Release: `2026.05.17`

This directory contains the repo-owned Jade Symphony skills used by local Codex
and Gemini operator sessions. The suite is intentionally versioned in the repo
so skill behavior can be reviewed with workflow docs, prompts, and CLI changes.

## Install Or Preview

Preview the detected local targets without writing:

```bash
node scripts/install-jade-symphony-skills.js --dry-run
```

Install or update after an interactive confirmation:

```bash
node scripts/install-jade-symphony-skills.js
```

Install non-interactively only after choosing explicit targets:

```bash
node scripts/install-jade-symphony-skills.js \
  --codex-dir "$HOME/.codex/skills" \
  --gemini-dir "$HOME/.gemini/local-skills" \
  --yes
```

Validate active local copies against the repo-owned suite:

```bash
node scripts/install-jade-symphony-skills.js --validate
```

The installer detects:

- Codex target from `CODEX_HOME/skills`, then `$HOME/.codex/skills`.
- Gemini target from `GEMINI_HOME/local-skills`, then `$HOME/.gemini/local-skills`.

Use `--skip-codex`, `--skip-gemini`, `--codex-dir`, or `--gemini-dir` to make
the target set explicit. Normal install mode shows every target and requires
operator confirmation before writing.

## Packaged Skills

- `jade-symphony-issue-forge`
- `jade-symphony-issue-forge-reflect`
- `jade-symphony-manual-main`
- `jade-symphony-manual-review`
- `jade-symphony-human-review`
- `jade-symphony-manual-merge`
- `jade-symphony-doctor`

Human Review briefs the operator after Review Agent pass evidence, guides
operator-owned UAT, records a structured decision note, and routes only after
explicit confirmation. Accepted Human Review goes to `Merging`, not `Done`.

The Doctor skill is a stub slot for operator triage and install-health checks.
Full automatic doctor install-health repair remains out of scope for this
release and belongs to follow-up issue #256.
