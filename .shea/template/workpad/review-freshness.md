## Review Freshness

- Issue: {{issue_ref}}
- Stale reason: {{stale_reason}}
- Rework class: {{rework_class}}
- Prior head SHA: `{{prior_head_sha}}`
- Current head SHA: `{{current_head_sha}}`
- Prior base SHA: `{{prior_base_sha}}`
- Current base SHA: `{{current_base_sha}}`
- Prior Human Review still valid: `{{prior_human_review_valid}}`
- Human re-review required: `{{human_rereview_required}}`
- Main-agent target state: `{{main_agent_target_state}}`
- Authorized next state after review freshness evidence: `{{authorized_next_state}}`
- Decision: {{decision}}
- Rationale: {{rationale}}

### Changed Files

{% if changed_files == "" %}- None recorded.{% else %}{% assign files = changed_files | split: record_separator %}{% for file in files %}- `{{file}}`
{% endfor %}{% endif %}
### Patch Summary

{{patch_summary}}

### Authority Boundary

- This freshness report is evidence, not an automatic approval.
- Main implementation agent still stops at `Agent Review`.
- `Human Review` remains reserved for an independent Review Agent or human-authorized workflow.
