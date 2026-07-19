## Shea Symphony Agent Review Run

- Generated at: `{{generated_at}}`
- Issue: {{issue_ref}} {{issue_title}}
- Worker key: `{{worker_key}}`
- Reviewer backend: `{{reviewer_backend}}`
- Lane: `review`
- Actor role: `review_agent`
- Run ID: `{{run_id}}`
- Input state: `Agent Review`
- Job state: `{{job_state}}`
- Decision: {{decision}}
- Target state after review routing: `{{target_state}}`
- Result: `{{result}}`
{{pr_line}}
{{artifact_line}}
{{ledger_line}}
- Evidence summary: {{evidence_summary}}

### Review Attempt {{job_id}}

{{attempt_details}}

{{operator_action_section}}
{{gemini_health_section}}
{{usage_limit_section}}
{{inconclusive_section}}
### Review Response

{{agent_review_note}}

{{findings_section}}

{{stdout_section}}
{{stderr_section}}
{{pass_evidence_section}}
