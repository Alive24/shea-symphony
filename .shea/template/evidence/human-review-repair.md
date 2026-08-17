## Shea Symphony Doctor Triage

- Generated at: `{{generated_at}}`
- Issue: {{issue_ref}} {{issue_title}}
- Lane: `doctor`
- Actor role: `doctor`
- Actor: `shea-symphony doctor`
- Run ID: `doctor-human-review-repair`
- Input state: `{{input_state}}`
- Target state after repair: `Agent Review`
- Result: `repair_recorded`
- PR evidence: `not recorded`
- Violation: `{{violation_code}}`
- Message: {{message}}
- Repair: {{repair}}
- Evidence summary: invalid Human Review boundary repair evidence recorded before tracker mutation.

### State Boundary
- Main implementation is moving this issue back to `Agent Review`.
- This repair does not set `Human Review`; that state requires independent Review pass evidence.
