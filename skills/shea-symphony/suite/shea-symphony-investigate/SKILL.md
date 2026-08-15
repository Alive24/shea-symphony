---
name: shea-symphony-investigate
description: Use when exploring Shea Symphony symptoms, rough ideas, workflow doubts, surprising tracker or runtime behavior, or possible issue candidates before deciding whether to forge or rework a GitHub issue. Gather evidence, separate hypotheses, classify the problem, and recommend whether to keep investigating, use Issue Forge, route Human Review or Doctor, or take no action.
metadata:
  short-description: Shea Symphony pre-Forge investigation
  suite-version: 2026.08.15
---

# Shea Symphony Investigate

Explore ambiguous Shea Symphony observations before they become executable work.
Use this skill to protect the investigation phase: gather evidence, keep
multiple hypotheses alive, and decide whether the next step is Issue Forge,
Doctor, Human Review, another evidence pass, or no action.

This skill does not replace `$shea-symphony-issue-forge`. It hands off to Forge
only after the problem is clear enough to draft a bounded issue contract.

## Repository

Default repository:

```bash
/Volumes/Bohemialive/GitHub/shea-symphony
```

Canonical workflow:

```bash
.shea/workflows/shea-symphony.md
```

## Operating Rule

Investigate first, classify second, route third.

- Do not start by drafting an issue body.
- Do not force a single interpretation before checking evidence.
- Do not mutate tracker state, create issues, rework issues, install skills, or
  change code unless the operator explicitly switches out of investigation and
  confirms the bounded write.
- Prefer read-only CLI commands, local code search, runtime artifacts, logs,
  Project readbacks, PR metadata, and existing docs before speculation.
- Ask 1-3 focused questions only when local evidence cannot resolve the
  ambiguity or when the next read would be risky or expensive.
- Keep the live conversation in the operator's language. Preserve exact command
  names, file paths, issue states, labels, and skill names in English.

## Evidence Sources

Choose the smallest useful evidence set for the question. Common Shea Symphony
reads include:

```bash
cargo run -- project state .shea/workflows/shea-symphony.md
cargo run -- project issue .shea/workflows/shea-symphony.md '#<issue>' --json
cargo run -- doctor .shea/workflows/shea-symphony.md
cargo run -- debug .shea/workflows/shea-symphony.md
cargo run -- autopilot plan .shea/workflows/shea-symphony.md
```

Use repository inspection when the question concerns implementation or wording:

```bash
rg "<term>"
rg --files
git status --short
git diff -- <path>
```

Use runtime artifacts when the question concerns live lane behavior, recovery,
claims, app state, or session continuity. Inspect only relevant files, such as:

- runtime state and session registry files;
- `logs/app-server/`;
- issue-specific workpads and artifacts;
- archived runtime snapshots when the latest state appears inconsistent.

For GitHub evidence, prefer read-only commands:

```bash
gh issue view <issue> --repo Alive24/shea-symphony --comments
gh pr view <pr> --repo Alive24/shea-symphony --json number,title,state,url,isDraft,baseRefName,headRefName,mergeStateStatus,reviewDecision,statusCheckRollup
```

## Classification Checklist

Separate observations into one or more of these classes before recommending a
route:

- `Projection or display mismatch`: UI, lane board, or summary text disagrees
  with the underlying tracker or runtime state.
- `Dispatchability mismatch`: an issue looks eligible but the lane loop cannot
  or should not pick it.
- `Workflow contract gap`: the intended human, Main, Review, or Merge behavior
  is not encoded clearly enough in docs, prompts, CLI, or skills.
- `Runtime or recovery issue`: sessions, claims, worktrees, or recovery state
  are stale, inconsistent, or unsafe.
- `Issue contract problem`: an existing issue body, dependency, UAT rule,
  parent/subissue shape, or guardrail needs revision.
- `Implementation bug`: code behavior is wrong and can likely be fixed through
  a normal issue or focused patch.
- `Operator wording or UX problem`: labels, action text, or briefings imply the
  wrong responsibility or workflow state.
- `Backlog candidate`: the idea is real but not urgent or not dispatchable yet.
- `Expected behavior or no action`: the evidence supports leaving the system as
  is.

For Shea-specific ambiguity, explicitly test these distinctions when relevant:

- display/projection vs actual dispatchability;
- lane-local recovery vs real workflow failure;
- issue-scoped evidence vs lane-wide leakage;
- local-only child-turn implementation vs parent-side tracker mutation;
- Human Todo semantics vs observational lane-board surfaces;
- abstraction boundary problem vs simple command-name replacement.

## Investigation Flow

1. Restate the investigation question in one sentence.
2. Gather the smallest read-only evidence set that can change the answer.
3. List observed facts separately from interpretations.
4. Maintain competing hypotheses until evidence rules them out.
5. Classify the problem using the checklist above.
6. Identify the narrowest next route:
   - continue investigation;
   - hand off to `$shea-symphony-issue-forge`;
   - hand off to `$shea-symphony-doctor`;
   - hand off to `$shea-symphony-human-review`;
   - run a manual lane skill;
   - record a backlog seed;
   - take no action.
7. If recommending Forge, provide a Forge-ready brief instead of creating the
   issue yourself.

## Output Shape

Keep the report concise. Use this shape by default:

```md
## Investigation Question

...

## Evidence Checked

- ...

## Observed Facts

- ...

## Current Hypotheses

1. ...
2. ...

## Classification

- ...

## Recommendation

- Route: ...
- Why: ...
- Next read or action: ...

## If Forged

- Suggested issue goal:
- Scope:
- Out of scope:
- Evidence to include:
- Open ambiguity:
```

Omit `If Forged` when the recommendation is clearly Doctor, Human Review, no
action, or continued investigation without an issue candidate.

## Forge Handoff

When the answer is ready for `$shea-symphony-issue-forge`, hand off a compact
brief:

- the investigation question and classification;
- evidence checked, with commands or paths;
- the recommended issue shape;
- remaining assumptions and why they are acceptable;
- suggested parent/subissue split if the work spans independent verification
  slices;
- whether the issue should start as `Backlog` or can be dispatchable `Todo`.

Do not run `forge create`, `forge rework`, or raw Project mutations from this
skill. Switch to `$shea-symphony-issue-forge` and require the normal explicit
operator confirmation before any issue creation or contract revision.
