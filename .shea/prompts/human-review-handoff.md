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
- Before preflight, UAT, or routing discussion, present a visible Human Review Brief in the operator's language with these fields:
  - Problem: what user, operator, or system problem the issue addresses.
  - Delivered change: what behavior changed and where.
  - Resulting effect: the observed before/after outcome; label intended or unevidenced effects explicitly.
  - Evidence: what Agent Review and current readbacks establish, including risks or gaps.
  - Human decision needed: the remaining human-owned check or acceptance choice and available routes.
- Include issue/PR identity and current state. Never omit a field; use `unknown`, `not evidenced`, or `not applicable`.
- Internal reasoning, tool output, links, freshness status, and test summaries do not satisfy this visible briefing requirement.
- If preflight changes the branch or evidence, re-present the affected fields before asking for UAT.
- Do not approve, reject, route, merge, or mutate Project state until the operator gives explicit approval.
- Preserve Shea lane boundaries and keep the operator-facing readback concise and source-grounded.
