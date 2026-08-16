## Shea Symphony Doctor Triage

- Generated at: `{{generated_at}}`
- Issue: {{issue_ref}} {{issue_title}}
- Lane: `doctor`
- Actor role: `doctor`
- Actor: `shea-symphony doctor`
- Run ID: `{{run_id}}`
- Input state: `{{input_state}}`
- Target state after repair: `{{target_state}}`
- Result: `{{result}}`
- Requested action: `{{action}}`
{{extra_lines}}
- Evidence summary: {{evidence_summary}}

### Doctor Findings

{{doctor_findings}}

### State Boundary

- Doctor repair records evidence before any tracker mutation.
- This repair does not delete worktrees, discard local work, or bypass Review/Merge authority.
