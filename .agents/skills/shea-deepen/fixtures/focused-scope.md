# Focused Operator Scope

## Prompt

Deepen the architecture around `src/lanes/review_loop`.

## Expected behavior

- Bind the named area before scanning and skip hot-spot inference.
- Read its callers, focused tests, instructions, and relevant authoritative docs.
- Keep all candidates inside the selected Review-loop area.
- Produce only the ignored local report; modify no source or tracker state.
