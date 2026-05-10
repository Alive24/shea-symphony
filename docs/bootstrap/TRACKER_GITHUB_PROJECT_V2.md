# GitHub Project v2 Tracker Spec

Status: Bootstrap v0

## Tracker Choice

The first Jade Symphony implementation uses GitHub Project v2 as the concrete
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
  repo: jade-symphony
  project_owner: Alive24
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
    marker: "<!-- jade-symphony-workpad -->"
```

## Adapter Operations

The GitHub Project v2 adapter must support:

- `list_dispatchable_issues(filter)`
- `get_issue(issue_ref)`
- `set_state(issue_ref, normalized_state)`
- `upsert_workpad(issue_ref, markdown)`
- `create_follow_up_issue(input)`
- `add_issue_to_project(issue_id)`
- `link_pull_request(issue_ref, pr_ref)`
- `list_linked_pull_requests(issue_ref)`

## Status Field Handling

Project v2 single-select status updates require option IDs.

The adapter should:

1. Load the ProjectV2 fields.
2. Find the configured `Status` single-select field.
3. Cache the field ID and option IDs for the current run.
4. Map normalized states to configured option names.
5. Use the option ID when updating status.

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
<!-- jade-symphony-workpad -->
```

Rules:

- Create one workpad comment if none exists.
- Reuse the existing marker comment if present.
- Do not use the issue body for runtime progress.
- Keep PR links in GitHub-native references when possible.
- Keep a concise final handoff in the workpad before state transition.

## Linear Compatibility Rule

Every GitHub-specific field must map cleanly to a future Linear adapter:

- ProjectV2 Status -> Linear issue state.
- GitHub Issue body -> Linear issue description.
- GitHub Issue comment workpad -> Linear comment workpad.
- GitHub Issue assignees -> Linear assignee.
- GitHub Project -> Linear project.

