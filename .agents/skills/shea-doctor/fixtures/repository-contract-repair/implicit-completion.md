# Implicit Completion Boundary Fixture

## Input

Target: `prompts/main.md`

```markdown
Implement the accepted issue scope and run the relevant checks.
Create a pull request for the finished work.
```

Run evidence: the worker reported completion after a repairable lint failure
and before publishing a ready linked PR.

## Observed evidence

- The contract has no terminal rule for repairing and rerunning failed checks.
- The contract does not bind completion to PR and handoff obligations.

## Expected classification

- `missing_completion_invariant`

## Expected disposition

`proposal`: add one concise rule requiring in-scope verification repair and
rerun, then forbid completion until required verification, ready-PR, linkage,
workpad, and Agent Review handoff obligations pass.
