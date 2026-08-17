## Shea Symphony Rework Run

- Generated at: `{{generated_at}}`
- Issue: {{issue_ref}} {{issue_title}}
- Lane: `main`
- Actor role: `human_review_revision`
- Actor: `operator`
- Run ID: `forge-rework`
- Input state: `Human Review`
- Target state after run: `Rework`
- Result: `rework_revision_recorded`
- PR: `{{pr}}`
- Replacement Rework title/status: `{{rework_title}}` / `Rework`
- Operator confirmation: {{operator_confirmation}}
- Evidence summary: operator confirmation, replacement contract, and readback evidence recorded before the final state mutation.

### Rework Direction
{{evidence}}

### Verification Readback
{{readbacks}}

### Role Boundary
- Main may claim `Rework`, update the canonical Main workpad, and stop at `Agent Review`.
- `Human Review` remains reserved for independent Review pass evidence.
