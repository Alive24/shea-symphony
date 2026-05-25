# GitHub Project v2 Tracker Spec

Status: Bootstrap v0

## Tracker Choice

The first Shea Symphony implementation uses GitHub Project v2 as the concrete
tracker. Linear remains a required future adapter, so all GitHub-specific logic
must stay inside the tracker adapter.

## Domain Split

GitHub Project v2 has two relevant objects:

- GitHub Issue: work content, assignees, labels, discussion, linked PRs, and
  workpad comments.
- ProjectV2 Item: workflow status and project-specific fields.

Do not treat the GitHub Issue itself as the workflow state object.

## Required Configuration

```yaml
tracker:
  kind: github_project_v2
  owner: Alive24
  repo: shea-symphony
  project_owner: Alive24
  project_owner_type: user # optional: user or organization
  project_number: 1
  status_field: Status
  state_map:
    backlog: Backlog
    todo: Todo
    need_to_clarify: Need to Clarify
    in_progress: In Progress
    need_human_input: Need Human Input
    agent_review: Agent Review
    human_review: Human Review
    rework: Rework
    merging: Merging
    done: Done
  assignee_filter:
    source: issue_assignees
    allow_unassigned: false
    assignees: []
  workpad:
    source: issue_comment
    marker: "<!-- shea-symphony-workpad -->"
```

`project_owner_type` is optional for compatibility with older workflows. When
it is omitted, the adapter keeps the legacy organization-then-user owner
resolution path. When it is set to `user` or `organization`, ProjectV2 metadata
and item reads use only that owner type for the configured run.

## Adapter Operations

The GitHub Project v2 adapter must support:

- `list_dispatchable_issues(filter)`
- `get_issue(issue_ref)`
- `set_state(issue_ref, normalized_state)`
- `upsert_workpad(issue_ref, markdown)`
- `add_issue_comment(issue_ref, markdown)` for append-only lane timeline
  evidence
- `create_follow_up_issue(input)`
- `add_issue_to_project(issue_id)`
- `link_pull_request(issue_ref, pr_ref)`
- `list_linked_pull_requests(issue_ref)`

## Status Field Handling

Project v2 single-select status updates require option IDs.

The adapter should:

1. Load the ProjectV2 fields through REST first when available.
2. Find the configured `Status` single-select field.
3. Cache the field ID and option IDs for the current run.
4. Map normalized states to configured option names.
5. Use the option ID when updating status.
6. Refresh cached metadata once when a required field or option lookup fails,
   then report the missing field/option explicitly if the refreshed metadata is
   still stale.

The cache is process-local to the tracker client. It covers the Project node ID,
REST owner kind, configured `Status` field, Status option IDs, supported
single-select/text/number/date field metadata, and lane claim text fields such
as `Main Agent`, `Review Agent`, and `Merging Agent` when those fields exist.
It is not a daemon and does not persist across CLI invocations.

## REST-First Field And Item Access

When GitHub REST Projects v2 exposes the required data, the adapter should use:

- `GET /orgs/{org}/projectsV2/{project_number}` or
  `GET /users/{username}/projectsV2/{project_number}` for Project metadata.
- `GET /orgs/{org}/projectsV2/{project_number}/fields` or the user equivalent
  for field metadata and single-select options.
- `GET /orgs/{org}/projectsV2/{project_number}/items` or the user equivalent
  with requested REST field IDs for item field overlays.
- `PATCH .../items/{item_id}` with a `fields` array for supported Status and
  lane-claim field updates.

REST pagination must be consumed completely. The CLI path uses `gh api
--paginate --slurp` for REST array endpoints and flattens every returned page
before parsing fields or items.

GraphQL remains the fallback for unsupported REST data, missing REST item IDs,
missing REST field IDs, item addition, workpad/comment mutation, and rich issue
relationships that REST does not expose in the required tracker shape. Fallback
reasons should be visible in code paths, tests, or operator diagnostics rather
than silently dropping fields.

## Status Write Authority

Status mutation must respect actor authority:

- Main implementation agent may set `In Progress`, `Need to Clarify`,
  `Need Human Input`, `Agent Review`, and `Rework` when those transitions follow
  the workflow.
- Main implementation agent must never set `Human Review`.
- Independent Review Agent may set `Human Review` only after an asynchronous
  review passes and evidence is recorded for ordinary issues or parent final
  issues. Routine native subissue pass evidence routes to `Merging`; direct
  subissue Human Review requires an explicit exception in issue or Project
  evidence.
- Independent Review Agent must set `Rework` when confirmed findings require
  changes.
- Failed, timed out, inconclusive, or backend-unavailable review must not set
  `Human Review`; it should remain in `Agent Review` or move to
  `Need Human Input`.

## Dispatch Eligibility

v0 should dispatch only real GitHub Issues that are already in the configured
Project v2 project.

Do not dispatch:

- Draft project items.
- Pull request project items.
- Issues without a matching ProjectV2 item.
- Issues whose status field is missing or unmapped.
- Issues assigned to a non-allowed assignee.
- Unassigned issues when `allow_unassigned` is false.

## Workpad

The workpad is a GitHub Issue comment with a stable marker:

```html
<!-- shea-symphony-workpad -->
```

Rules:

- Create one workpad comment if none exists.
- Reuse the existing marker comment if present.
- Do not use the issue body for runtime progress.
- Keep PR links in GitHub-native references when possible.
- Keep a concise final handoff in the workpad before state transition.

The workpad is not a shared lane ledger. Main implementation keeps exactly one
persistent `Main Agent Workpad` comment and updates it in place. Review,
Rework trigger diagnostics, Merge, Human Review, and Doctor triage or repair
records use standalone append-only issue comments. Those timeline comments
preserve chronology and must include timestamp, run id, lane, actor, input
state, target state, PR when relevant, result, and evidence summary.

## Linear Compatibility Rule

Every GitHub-specific field must map cleanly to a future Linear adapter:

- ProjectV2 Status -> Linear issue state.
- GitHub Issue body -> Linear issue description.
- GitHub Issue comment workpad -> Linear comment workpad.
- GitHub Issue assignees -> Linear assignee.
- GitHub Project -> Linear project.
