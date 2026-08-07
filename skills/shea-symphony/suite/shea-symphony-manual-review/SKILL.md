---
name: shea-symphony-manual-review
description: Use when the operator wants to trigger one Shea Symphony review for a named issue through the external Review backend configured by the active workflow, including an explicitly operator-authorized standalone implementation with a ready PR. Resolve the active CLI and workflow, validate or prepare the targeted handoff, invoke `review once`, and read back the result. Do not review the code or manufacture manual review evidence in the current agent.
metadata:
  short-description: Trigger one workflow-backed external review
  suite-version: 2026.08.07
---

# Shea Symphony Manual Review

Trigger one operator-selected Review run through the external backend configured
by the repository workflow. The current task is the operator-side launcher and,
when explicitly requested, the preparer of a safe standalone review handoff. The
configured backend owns diff inspection, review judgment, evidence, and routing.

Do not perform the review in the current agent. Do not replace a failed external
launch with direct diff inspection, a current-session conclusion, or fake/manual
review evidence.

## Bind the Active Repository

Never depend on hard-coded user names, volumes, checkout paths, models, or
backend commands. From the target repository root:

1. Read the profile selected by `SHEA_SYMPHONY_APP_PROFILE_PATH`, when set.
   Otherwise prefer `.shea/app-profile.local.json` over
   `.shea/app-profile.json`. Use it for `workflow_path` and `cli_path`.
2. Otherwise prefer `.shea/workflows/shea-symphony.md` and
   `.shea/bin/shea-symphony` when they exist.
3. Resolve both paths to absolute paths and verify the CLI with `--help` before
   any mutation.
4. Read the workflow's repository, Project, workspace root, base branch,
   Review prompt, and `review_lane` configuration.

Use concise shell variables in subsequent commands:

```bash
SHEA_CLI="<resolved-cli-path>"
SHEA_WORKFLOW="<resolved-workflow-path>"
ISSUE="#<number>"
```

Do not substitute `cargo run` without confirming that it builds the operational
CLI selected by the repository profile. Do not call AGY, Gemini, Claude, Codex,
or another reviewer command directly; Shea must launch the configured backend.

## External Backend Gate

Require `review_lane.backend` to select a real external backend supported by the
resolved CLI. Reject `fake`, `fake-reviewer`, fixture-only configuration, an
empty backend, or an unrecognized backend. Let CLI configuration validation
enforce the backend-specific command, model, approval, sandbox, and transport
contract.

For example, `agy-cli` launches the configured AGY subprocess and
`codex-app-server` launches a fresh independent Codex Review thread. Neither
means that the current agent may act as reviewer.

If the external executable, authentication, model, policy, sandbox, or
transport is unavailable, stop with the backend error and required operator
action. Never fall back to local review.

## Targeted Preflight

Use targeted reads for the named issue:

```bash
"$SHEA_CLI" project issue "$SHEA_WORKFLOW" "$ISSUE" --json
"$SHEA_CLI" project inspect "$SHEA_WORKFLOW" "$ISSUE" --lane review
"$SHEA_CLI" workspace show "$SHEA_WORKFLOW" "$ISSUE"
```

Use `gh issue view` and `gh pr view` only for the named issue and linked PR.
Confirm before launch that:

- Project Status is `Agent Review`, unless the operator explicitly requests a
  supported re-review or standalone handoff preparation;
- the PR closes or clearly links to the issue and is ready rather than draft;
- the Main handoff and canonical issue workspace are present and consistent, or
  one unambiguous standalone workspace and ready PR satisfy the fast path below;
- no active `Review Agent` claim or conflicting review job already owns the
  issue;
- the workflow selects a supported non-fake external Review backend.

For routine native subissues, passing review routes to `Merging`, not `Human Review`;
the parent owns final Human Review unless a recorded exception says otherwise.

Do not use a whole-Project scan or an all-lane loop for routine preflight. Stop
on ambiguous issue, PR, workspace, claim, or backend identity instead of
guessing.

## Operator-Authorized Standalone Fast Path

Use this path only when the operator explicitly asks to review a named
standalone implementation that did not run through Main. Do not require a Main
claim, assignee, automated Main workpad, or prior `Agent Review` state merely to
launch that manual review.

Before mutating anything, require all of the following:

- exactly one open, ready PR is explicitly named or unambiguously associated
  with the issue;
- the PR head matches the selected clean issue worktree and pushed commit;
- the PR base matches the issue's explicit target branch, when present,
  otherwise the workflow base;
- the issue is not terminal, has no active lane claim, and has no active or
  queued Review job;
- dependency and native-subissue gates are terminal or non-blocking;
- the external Review backend passes the gate above.

Then prepare the handoff through supported Shea commands in this order:

```bash
"$SHEA_CLI" project link-pr "$SHEA_WORKFLOW" "$ISSUE" "#<pr>" --write
"$SHEA_CLI" workspace adopt "$SHEA_WORKFLOW" "$ISSUE" "<worktree>" --write
"$SHEA_CLI" project issue "$SHEA_WORKFLOW" "$ISSUE" --json
"$SHEA_CLI" workspace show "$SHEA_WORKFLOW" "$ISSUE"
"$SHEA_CLI" project set-state "$SHEA_WORKFLOW" "$ISSUE" agent_review --write
"$SHEA_CLI" project issue "$SHEA_WORKFLOW" "$ISSUE" --json
```

Treat the PR linkage and Workspace Evidence as the standalone equivalent of a
Main handoff. Preserve the issue's assignee and do not manufacture a Main Agent
claim. Make `Agent Review` the final preparation mutation, then perform only
readback before launching `review once`. If linkage, adoption, status write, or
readback fails, stop without launching the backend.

For a PR targeting an explicit non-default base, GitHub may not expose a native
closing relationship. If `project link-pr` writes its one fallback comment but
reports missing native readback, do not retry the mutation. Continue only when
targeted `project issue` readback exposes exactly that PR number and URL as
`fallback_diagnostic` and targeted `gh pr view` independently confirms the
expected ready state, base, head branch, and head commit. Record the native-link
gap. Any missing, additional, draft, closed, or mismatched PR remains a hard
stop.

## Launch One External Review

Run exactly one configured Review backend for the named issue:

```bash
"$SHEA_CLI" review once "$SHEA_WORKFLOW" "$ISSUE" --write
```

`review once` owns prompt rendering, backend launch, structured output parsing,
review evidence, checklist updates, and result routing. Do not separately run
`review claim`, `review pass`, `review reject`, or `review-clear-claim`; those
commands belong to the distinct manual-evidence path and would mix ownership
with this workflow-backed run.

Do not inspect the PR diff to supplement or override the backend result. Reading
metadata and generated review evidence for launch verification and reporting is
allowed.

## Read Back

After `review once` returns, perform only targeted readback:

```bash
"$SHEA_CLI" review status "$SHEA_WORKFLOW" --issue "$ISSUE" --recent 3 --verbose
"$SHEA_CLI" project issue "$SHEA_WORKFLOW" "$ISSUE" --json
```

Report the external backend identity, terminal job result, evidence location,
and resulting Project state. If the command fails before durable evidence or
routing is visible, report the external backend failure without inventing a
review outcome.

## Safety

- Do not merge or force-push.
- Do not edit implementation code.
- Do not run `review fake`.
- Do not create a manual `Review Agent` claim or evidence file.
- Do not call `review pass`, `review reject`, or raw Project GraphQL mutation.
  The supported `project set-state` command is allowed only for the explicit
  standalone fast path above.
- Do not let the current agent review the diff, execute reviewer verification,
  classify findings, update checklists, or choose the next state.
- Do not retry configuration or authentication failures in a loop.
- Do not claim automatic-loop worker-pool, concurrency, or retry semantics;
  this skill intentionally triggers the targeted `review once` surface.
- Do not present this targeted launcher as `autopilot plan` or `autopilot loop`;
  those are separate all-lane planning and foreground-supervision surfaces.
