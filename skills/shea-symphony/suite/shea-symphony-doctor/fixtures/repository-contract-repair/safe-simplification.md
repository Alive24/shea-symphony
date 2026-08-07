# Safe Simplification Fixture

## Input

Targets: `prompts/review.md`, `templates/review.md`

The same three-sentence Review stop boundary appears in both files. Renderer
readback proves `prompts/review.md` is authoritative and the template paragraph
is never rendered or consumed.

## Observed evidence

- The template paragraph is unreachable.
- The prompt retains the independent Review and lane-stop invariant.

## Expected classification

- `duplicated_instruction`
- `stale_or_unreachable_text`

## Expected disposition

`proposal`: remove only the unreachable template copy after path-scoped
confirmation. Preserve the authoritative prompt and verify template rendering.
Unrelated template customizations must remain byte-for-byte unchanged.
