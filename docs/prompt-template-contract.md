# Prompt Template Contract

Status: strict subset, not full Liquid compatibility.

Jade Symphony renders the workflow prompt before launching an agent backend. The
official reference requires strict template rendering for `issue` and `attempt`
context and records Liquid-compatible semantics as sufficient. Jade Symphony currently
implements a deliberate subset so prompt failures are easy to diagnose during
dogfood.

## Supported Context

- `issue.*`: fields from the normalized `TrackerIssue` model.
- `attempt`: optional numeric attempt count; renders empty when no attempt is
  present.

Examples:

```liquid
Work on {{ issue.identifier }}: {{ issue.title }}
This is attempt {{ attempt }}.
```

String values render as plain text. Non-string JSON values, such as arrays,
numbers, objects, and booleans, render as JSON text.

## Supported Tags

Jade Symphony supports one level of basic conditionals:

```liquid
{% if issue.description %}
{{ issue.description }}
{% else %}
No description provided.
{% endif %}
```

Truthiness follows JSON-like behavior:

- `null`, empty strings, empty arrays, empty objects, `false`, and numeric zero
  are falsey.
- non-empty strings, arrays, objects, `true`, and non-zero numbers are truthy.

## Strict Failures

The renderer fails instead of guessing when it sees:

- unknown variables, such as `{{ issue.missing_field }}`;
- variables outside the supported root objects, such as `{{ user.name }}`;
- unsupported tags, such as `{% for %}`, `{% assign %}`, `{% include %}`, or
  filters.

This strictness is intentional. It keeps malformed prompts from silently
reaching an agent backend with missing context.

## Parity Gap

Full Liquid-compatible prompt rendering remains a parity roadmap item. Until a
vetted Liquid engine or complete parser is added behind the prompt boundary,
workflow prompts should stay within the subset above.
