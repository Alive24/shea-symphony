# Bloated and Contradictory Contract Fixture

## Input

Target: `prompts/main.md`

```markdown
Run required tests before completion.
You may report completion even when required tests fail.
Run required tests before completion.
Move the issue to Agent Review only after a ready linked PR exists.
The Main worker may independently approve Human Review.
```

## Observed evidence

- The first rule is duplicated byte-for-byte.
- The second rule contradicts required verification.
- The final rule leaks Human Review authority into Main.

## Expected classification

- `duplicated_instruction`
- `contradictory_instruction`
- `lane_leakage`

## Expected disposition

`proposal`: remove the duplicate, remove the completion contradiction, and
remove or relocate the Human Review rule. Preserve required verification,
ready-PR/linkage, and Agent Review handoff invariants.
