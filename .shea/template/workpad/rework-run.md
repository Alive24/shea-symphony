## Shea Symphony Rework Run

- Generated at: `<YYYY-MM-DD HH:MM:SS GMT>`
- Issue: #<issue> <title>
- Lane: `review` | `human_review` | `merge` | `main`
- Actor role: `review_agent` | `human_review_revision` | `merge_agent` | `implementation_agent`
- Actor: <worker or operator>
- Run ID: `<run-id>`
- Run type: `rework_trigger_diagnostic`
- Trigger: Agent Review finding | Human Review feedback | merge-lane repair
- Input state: `Agent Review` | `Human Review` | `Merging`
- Target state after run: `Rework`
- Result: Rework recorded | Blocked | Diagnostic only
- PR: #<pr> <url>
- Evidence summary: <short summary of evidence checked>

### Purpose

- Record why this issue entered `Rework`.
- Preserve the review/human/merge finding that the next Main Agent round must address.
- Do not use this comment as the canonical Main implementation workpad.

### Rework Trigger

- ...

### Required Main Follow-Up

- ...

### Evidence

- ...

### Boundary

- Main-lane rework implementation must update the existing `Main Agent Workpad` in place.
- This comment is append-only timeline evidence and must not overwrite or replace the Main Agent Workpad.
