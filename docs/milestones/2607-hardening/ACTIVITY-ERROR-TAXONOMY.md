# Activity Error Taxonomy

Status: Draft

## Purpose

Define how 2607 Temporal Activities classify failures so `IssueWorkflow` can
retry, wait, reconcile, or request human input without every Activity inventing
its own routing policy.

This taxonomy should migrate existing Shea Symphony semantics rather than
replace them.

Existing inputs:

- tracker `ProjectStateFailureKind`;
- review backend health diagnostics and recovery policy;
- Codex/app-server completion, failure, cancellation, input-required, usage
  limit, and timeout signals;
- merge repair `retryable` versus `Need Human Input` outcomes;
- doctor findings that already distinguish automatic checks from human triage.

## Principle

Activities report typed outcomes. `IssueWorkflow` decides the next durable
state.

Do not encode tracker transitions directly inside arbitrary error handlers.
Do not classify every failure as retryable. Do not classify every failure as
`Need Human Input`.

## Outcome Classes

Use these normalized outcome classes across Activities.

### `success`

The Activity completed the requested work and returned a normal result.

Workflow behavior:

- continue to the next Workflow step;
- commit tracker transition when the state boundary requires it.

### `already_applied`

The requested side effect was already present on readback.

Examples:

- tracker state already equals target;
- project field already matches;
- workpad marker is already present;
- PR is already merged.

Workflow behavior:

- treat as successful for idempotent side effects;
- record summary evidence;
- continue.

### `retryable`

The failure is likely transient and can be retried by Temporal Activity retry
policy.

Examples:

- network errors;
- HTTP 5xx;
- temporary backend outage;
- retryable command timeout;
- interrupted but cleaned-up merge repair attempt;
- recoverable app-server/session transport failure.

Workflow behavior:

- let Temporal retry the Activity according to policy;
- heartbeat progress when long-running;
- after retry exhaustion, convert to `need_human_input` or `retry_later`
  according to Activity contract.

### `wait_and_retry`

The system should wait before retrying, usually because the provider gave a
rate-limit, quota, or capacity signal.

Examples:

- tracker rate limit;
- review quota/rate limit;
- Codex usage limit with a retry window;
- provider asks to retry after a delay.

Workflow behavior:

- schedule durable timer or Activity retry with delay;
- keep ownership visible in the current state;
- do not move to `Need Human Input` unless repeated waits exceed policy or the
  required human action is explicit.

### `need_human_input`

The workflow cannot safely or usefully continue without operator input.

Examples:

- missing secret or credential;
- auth failure;
- command not installed or not executable;
- unsupported model or invalid backend config;
- policy or allowed-tools refusal;
- dangerous or destructive operation needing approval;
- semantic uncertainty;
- unsafe merge repair precondition;
- untrusted local environment;
- unrecoverable local state ambiguity.

Workflow behavior:

- transition through `TrackerTransitionActivity` to `Need Human Input`;
- store structured `WaitingState`;
- include one concrete requested action and artifact refs.

### `conflict`

The Activity detected a state conflict rather than a transient failure.

Examples:

- tracker state changed outside `TrackerTransitionActivity`;
- active runtime state disagrees with tracker state;
- branch/PR evidence no longer matches issue state;
- another worker owns the same human-visible lane claim.

Workflow behavior:

- do not guess;
- if the conflict blocks active work, enter `Need Human Input` with reason
  `tracker_state_conflict` or a more specific enum;
- if the conflict is externally terminal, stop or reconcile according to
  `IssueWorkflow` policy.

### `rejected`

The Activity completed and produced a negative business decision.

Examples:

- contract check rejects Todo readiness;
- Agent Review finds actionable issues;
- human review requests changes;
- quality gate rejects backlog promotion.

Workflow behavior:

- route through the normal state graph, such as `Todo -> Need to Clarify`,
  `Agent Review -> Rework`, or `Backlog` remaining in shaping;
- do not treat this as infrastructure failure.

### `terminal_noop`

The requested work is no longer needed because the issue is already terminal or
explicitly cancelled.

Examples:

- tracker shows `Done`;
- issue was cancelled by operator policy;
- PR was already merged and terminal cleanup is already recorded.

Workflow behavior:

- finalize local Workflow state if appropriate;
- do not retry;
- do not ask for human input unless evidence is inconsistent.

### `bug`

The code violated an invariant or encountered an impossible state.

Examples:

- invalid enum value in durable Workflow state;
- malformed Activity payload produced by Symphony code;
- missing artifact ref that the same Workflow just recorded;
- serialization/versioning bug.

Workflow behavior:

- fail fast as non-retryable;
- preserve artifacts and Workflow history;
- route to developer-facing diagnosis rather than normal operator NHI when
  possible.

## Existing Mapping

### Tracker Errors

Map existing `ProjectStateFailureKind` values as follows:

| Existing kind | Activity outcome |
| --- | --- |
| `Network` | `retryable` |
| `TransientBackend` | `retryable` |
| `RateLimit` | `wait_and_retry` |
| `ResourceLimit` | `wait_and_retry` or `need_human_input` after policy exhaustion |
| `Auth` | `need_human_input` |
| `Schema` | `need_human_input` |
| `PartialResponse` | `retryable` for reads, `need_human_input` if repeated or schema-like |
| `Payload` | `bug` or `need_human_input`, depending on whether payload came from Symphony or external tracker |
| `MissingCapability` | `need_human_input` or `rejected` for unsupported optional operation |
| `Unknown` | `retryable` once if transport-like, otherwise `need_human_input` |

Preserve readback recovery:

- if readback proves the side effect landed, return `already_applied` or
  `success`;
- if readback is uncertain, return `conflict` or retry according to the error
  kind.

### Review Backend Health

Map existing review recovery policy:

| Existing policy | Activity outcome |
| --- | --- |
| `WaitAndRetry` | `wait_and_retry` |
| `RetryWithBackoff` | `retryable` |
| `RequiresHumanInput` | `need_human_input` |

Review verdicts are not infrastructure failures:

- pass maps to `success`;
- actionable findings map to `rejected`;
- inconclusive review with missing evidence maps to `rejected` or
  `need_human_input` depending on whether the next action is implementation
  work or operator input.

### Codex/App-Server Runs

Codex implementation Activities should classify at the attempt boundary.

Recommended mapping:

- completed with valid handoff evidence: `success`;
- completed but review/handoff contract failed: `rejected` or
  `need_human_input`, depending on whether agent rework is possible;
- input-required approval unavailable: `need_human_input`;
- usage limit: `wait_and_retry`;
- stall timeout or transport interruption with clean retry: `retryable`;
- cancellation requested by operator: `terminal_noop` or `need_human_input`
  depending on Workflow policy;
- malformed app-server event: `retryable` once if transport-like, `bug` if the
  parser or protocol assumption is wrong.

Temporal should not model every Codex turn as a separate failure boundary.

### Merge And Semantic Fix

Recommended mapping:

- clean merge landed: `success`;
- PR already merged: `already_applied`;
- pending checks or unknown mergeability: `wait_and_retry`;
- recoverable merge transport failure with merged readback: `already_applied`;
- retryable merge-agent backend or cleaned-up verification failure:
  `retryable`;
- semantic uncertainty, unsafe precondition, dirty untrusted worktree,
  push-failing branch, or checks-failing blocker: `need_human_input`;
- review-requested implementation changes before merge: `rejected` to
  `Rework`, not merge-time error.

Merge-time semantic fix failure should route to `Need Human Input`, not
`Rework`, unless the Workflow explicitly decides the issue needs a new
implementation pass.

### Doctor

Automatic doctor checks are Activities.

Recommended mapping:

- read-only findings: `success` with finding artifacts;
- missing auth/config/tooling needed for diagnosis: `need_human_input`;
- transient tracker/local read failure: `retryable`;
- confirmed state conflict: `conflict`;
- human doctor triage result: enters Workflow through Signal or Update, not a
  direct Activity-side tracker mutation.

## Retry Policy Rules

Activity retry should be conservative:

- retry only transient infrastructure failures automatically;
- do not retry semantic uncertainty;
- do not retry missing credentials/configuration without human input;
- do not retry destructive operations unless idempotency and readback are
  explicit;
- prefer durable timers for rate limits and usage limits;
- include attempt count, next retry time, and artifact refs in Workflow state
  summaries or Activity progress.

## Need Human Input Contract

Every `need_human_input` result must provide:

- reason enum;
- reason detail;
- requested action;
- resume target;
- artifact refs;
- whether the operator can retry after fixing the issue.

The Workflow converts this result into structured `WaitingState` and commits the
tracker transition through `TrackerTransitionActivity`.

## Deliberately Not Chosen

Do not let Activities mutate tracker state directly when classifying failures.

Why:

- tracker writes must stay centralized;
- error classification should not bypass `IssueWorkflow`;
- App and operator state should remain queryable through Workflow state.

Do not treat provider/model failures as all equivalent.

What this gives up:

- a single simple "agent failed" bucket.

Why this is acceptable:

- quota/rate-limit, missing auth, policy refusal, semantic uncertainty, and
  malformed payloads need different recovery paths.

Do not keep retry counters in GitHub Project fields.

Use Temporal Activity attempts, Workflow state summaries, and artifact refs for
local retry detail. Keep tracker fields for human-visible workflow facts.

## Implementation Notes

- Activity result structs should include a normalized outcome class.
- Activity failures used for Temporal retry must be distinguishable from
  business rejections.
- Non-retryable failures should include a stable reason code.
- Repeated same-cause failures should compact evidence rather than appending
  unbounded tracker comments.
- Query handlers expose current outcome summaries; they do not reclassify raw
  logs on every App refresh.
