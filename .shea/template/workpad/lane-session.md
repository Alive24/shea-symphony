{% if lane == "main" %}## Shea Symphony Workpad{% elsif lane == "review" %}## Shea Symphony Agent Review Run{% else %}## Shea Symphony Merge Run{% endif %}

{% if backend == "tmux" %}### Local tmux Agent Session{% else %}### Local Agent Session{% endif %}
- Generated at: `{{generated_at}}`
- Issue: {{issue_ref}} {{issue_title}}
- Lane: `{{lane}}`
- Actor role: `{{actor_role}}`
- Actor: `{{actor}}`
- Run ID: `{{run_id}}`
- Input state: `{{input_state}}`
- Target state after run: `{{target_state}}`
- Result: `{{result}}`
- PR: `{{pr}}`
- Claim field: `{{claim_field}}` = `{{claim_value}}`
- Backend: `{{backend}}`
- Agent command: `{{agent_command}}`
- Session: `{{session_id}}`
- Pending session: `{{pending_session}}`
- Workspace: `{{workspace_path}}`
- Prompt artifact: `{{prompt_path}}`
- Session log: `{{log_path}}`
- Attach command: `{{attach_command}}`
- Git identity: `{{git_identity}}`
- Evidence summary: {% if backend == "tmux" %}tmux session, prompt artifact, log path, workspace, and claim metadata recorded.{% else %}backend session, prompt artifact, workspace, and claim metadata recorded.{% endif %}

{{message}}
