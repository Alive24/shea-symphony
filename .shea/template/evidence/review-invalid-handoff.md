## Shea Symphony Agent Review Run

### Agent Review Invalid Handoff
- Issue: {{issue_ref}} {{issue_title}}
- Lane: `review`
- Input state: `Agent Review`
- Target state after review routing: `unchanged`
- Actor role: `review_agent`
- Decision: `inconclusive_invalid_handoff`
- Reason: {{reason}}
- Review did not start because the Main handoff invariant is not satisfied.
- Draft PRs must be marked ready by Main or an operator-confirmed Doctor repair before normal Agent Review.
