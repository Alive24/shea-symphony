Use the shea-symphony-human-review skill for {{ issue.identifier }}.

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
- Read the current Project issue, linked PR, Agent Review evidence, workpad, completion criteria, and UAT contract.
- Present the review evidence, remaining human-owned checks, and available routing decisions for discussion.
- Do not approve, reject, route, merge, or mutate Project state until the operator gives explicit approval.
- Preserve Shea lane boundaries and keep the operator-facing readback concise and source-grounded.
