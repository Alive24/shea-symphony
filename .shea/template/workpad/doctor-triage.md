## Shea Symphony Doctor Triage

- Generated at: `<YYYY-MM-DD HH:MM:SS GMT>`
- Issue: #<issue> <title>
- Lane: `doctor`
- Actor role: `doctor`
- Actor: <doctor command or operator>
- Run ID: `<run-id or doctor-action-id>`
- Input state: <state>
- Target state after repair: <state or unchanged>
- Result: Routed | Repaired | Triage recorded | Blocked
- PR: #<pr> <url> | not recorded
- Requested action: <action>
- Evidence summary: <short summary of doctor findings and repair evidence>

### Doctor Findings

- ...

### Repair Evidence

- ...

### State Boundary

- Doctor records evidence before any tracker mutation.
- Doctor does not delete worktrees, discard local work, or bypass review/merge lane authority.
