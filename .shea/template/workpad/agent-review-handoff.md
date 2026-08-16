## Agent Review Handoff

- Issue: {{issue_ref}} {{issue_title}}
- Status: `{{status}}`
- Target state after handoff: `{{target_state}}`
- PR: `{{pull_request}}`
- Linked PR handoff verified: `{{project_pr_link_verified}}`
- PR draft: `{{pull_request_is_draft}}`
- Validation: {{validation_summary}}
- Last transition: {{last_transition}}

### Missing Handoff Evidence
{{missing}}

### Agent Review Handoff Invariant
- Main implementation stops at `Agent Review` and must never set `Human Review`.
- Independent Review owns review evidence and later Human Review routing.
