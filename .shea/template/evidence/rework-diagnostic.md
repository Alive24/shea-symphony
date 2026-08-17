{% if review_origin == "true" %}## Shea Symphony Agent Review Run{% else %}## Shea Symphony Rework Run{% endif %}

- Generated at: `{{generated_at}}`
- Issue: {{issue_ref}} {{issue_title}}
- Lane: `{{lane}}`
- Actor role: `{{actor_role}}`
- Run ID: `{{source}}`
- Run type: `review_rework_diagnostic`
- Input state: `{{input_state}}`
- Target state after run: `Rework`
- Result: `{{kind}}`
- Source: `{{source}}`
- Kind: `{{kind}}`
- Summary: {{summary}}
- Next action: {{next_action}}
- Evidence was recorded before moving the issue to `Rework`.
- Evidence summary: {{changed_file_count}} changed file(s), {{finding_count}} finding(s).
{% if pr_ref != "" %}- Pull request: `{{pr_ref}}`
- PR-specific context is captured here; mirror this note to the PR conversation when the active adapter supports PR comments.{% else %}- Pull request: `not recorded`{% endif %}
{% if review_artifact_path != "" %}- Review artifact: `{{review_artifact_path}}`{% endif %}
{% if review_ledger_path != "" %}- Review job ledger: `{{review_ledger_path}}`{% endif %}
{% if changed_files != "" %}
### Changed Files
{% assign files = changed_files | split: record_separator %}{% for file in files %}- `{{file}}`
{% endfor %}{% endif %}{% if findings != "" %}
### Findings
{% assign records = findings | split: record_separator %}{% for record in records %}{% assign fields = record | split: field_separator %}- {{fields[0]}}: {{fields[1]}} - {{fields[2]}}
{% endfor %}{% endif %}{% if command != "" %}
### Command

- `{{command}}`
{% endif %}{% if stdout != "" %}
### Stdout

<details>
<summary>Stdout</summary>

```text
{{stdout}}
```

</details>
{% endif %}{% if stderr != "" %}
### Stderr

<details>
<summary>Stderr</summary>

```text
{{stderr}}
```

</details>
{% endif %}
### Role Boundary

{% if review_origin == "true" %}- Review Agent records the independent review result and may route confirmed findings to `Rework`.
- This comment is append-only trigger evidence; it does not replace the canonical Main Agent Workpad.
- Main implementation agent repairs confirmed Rework in the existing Main Agent Workpad, then stops at `Agent Review`.{% else %}- This comment is append-only Rework diagnostic evidence; it does not replace the canonical Main Agent Workpad.
- Main implementation agent records implementation repair evidence in the existing Main Agent Workpad and then stops at `Agent Review`.{% endif %}
- `Human Review` remains reserved for independent Review Agent pass evidence.
