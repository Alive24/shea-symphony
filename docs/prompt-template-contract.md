# Prompt Template Contract

Status: strict Liquid-compatible rendering.

Shea Symphony renders workflow lane prompts, selected backend fragments, and
configured runtime templates before launching agents or writing evidence. Rendering uses the Rust
`liquid` engine with the standard Liquid tag and filter library, plus Shea's
strict external-context validation.

## Supported Context

Prompt templates receive:

- `issue.*`: fields from the normalized `TrackerIssue` model.
- `attempt`: optional numeric attempt count.

Workpad templates receive the named values supplied by the caller for that
workpad surface, such as `issue_ref`, `issue_title`, `run_id`, `target_state`,
or `evidence_summary`.

Backend fragments are repository Markdown. Static fragments receive no dynamic
context; Merge repair receives only typed conflict facts such as `pr_ref`,
`head_ref_name`, `expected_base`, and sanitized `mechanical_stderr`. JSON Schema
construction, output classification, claim identity, and Project mutation
remain code-owned enforcement.

Examples:

```liquid
Work on {{ issue.identifier }}: {{ issue.title }}
This is attempt {{ attempt }}.
```

```liquid
### Labels
{% for label in issue.labels %}
- {{ forloop.index }}. {{ label }}
{% endfor %}
```

## Liquid Tags And Filters

Templates may use Liquid-compatible syntax supported by the selected engine,
including conditionals, loops, assignment/capture locals, and standard filters.
Representative supported filters include:

- `default`
- `join`
- `size`
- string filters such as `strip`, `upcase`, and `truncate`
- array filters such as `first`, `last`, `sort`, and `uniq`

Template-local variables introduced by Liquid tags, such as `for label in ...`,
`assign`, and `capture`, are allowed. External variables must come from the
prompt or workpad context described above.

## Strict Failures

Rendering fails before agent launch or workpad evidence write when a template
contains:

- an unknown external variable, such as `{{ issue.missing_field }}` or
  `{{ user.name }}`;
- an unknown workpad placeholder, such as `{{ missing_handoff_value }}`;
- an unknown filter, such as `{{ issue.title | no_such_filter }}`;
- malformed Liquid syntax, such as an unclosed `{% if %}` or `{% for %}` block.

This strictness is intentional. Liquid compatibility must not silently drop
missing data, tolerate misspelled filters, or write partial workpad evidence.

## Validation Readback

`cargo run -- validate .shea/workflows/shea-symphony.md` reports:

- `prompt_renderer=strict-liquid-compatible`;
- `prompt_template_smoke.<lane>=pass` for each configured lane prompt;
- `backend_prompt_source.<id>=...` with the exact selected Markdown path;
- `workpad_template.<id>=... smoke=pass` for every configured or centralized
  workpad/evidence template;
- `resource_manifest=... groups=...` and each exact
  `resource_markdown_source=...` from the enabled closure.

Any parse error, unknown filter, or unknown external variable in a configured
prompt or workpad template makes validation fail.
