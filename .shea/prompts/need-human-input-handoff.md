Use the shea-symphony-doctor skill for {{ issue.identifier }}.

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
- Read the current Project issue, diagnostics, workpad, and relevant runtime evidence before drawing a conclusion.
- Identify the precise blocker and the smallest decision or approval needed from the operator.
- Discuss the diagnosis and recommendation before performing any repair or Project mutation.
- Preserve Shea lane boundaries and do not mutate Project state without explicit operator approval.
- Keep the operator-facing readback concise and source-grounded.
