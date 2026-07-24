Use the shea-symphony-issue-forge skill for {{ issue.identifier }}.

Context
- Issue: {{ issue.identifier }} {{ issue.title }}
- State: {{ issue.state }}
{% if issue.lane %}- Lane: {{ issue.lane }}
{% endif %}{% if issue.category %}- Category: {{ issue.category }}
{% endif %}{% if issue.worker_status %}- Worker status: {{ issue.worker_status }}
{% endif %}{% if issue.worker_detail %}- Worker detail: {{ issue.worker_detail }}
{% endif %}{% if issue.recommended %}- Recommended next read: {{ issue.recommended }}
{% endif %}{% if issue.evidence %}- Evidence: {{ issue.evidence }}
{% endif %}{% if issue.url %}- URL: {{ issue.url }}
{% endif %}
Instructions
- Read the current Project issue and nearby package context before recommending a contract.
- Discuss scope, dependencies, boundaries, and acceptance evidence with the operator before promoting or creating anything.
- Do not create issues, promote drafts, or mutate Project state without explicit operator approval.
- Keep the operator-facing readback concise and source-grounded.
