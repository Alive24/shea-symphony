Use the shea-symphony-human-review skill for {{ issue.identifier }}.

App context — treat these values as hints and refresh them before relying on them
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
Follow the installed skill as the sole authoritative Human Review contract. If
it is missing or outdated, stop and report onboarding/configuration drift; do
not reconstruct its behavior from this handoff.
