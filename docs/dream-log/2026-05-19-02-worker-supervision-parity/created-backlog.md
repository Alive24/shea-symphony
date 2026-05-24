# Created Backlog

## #324 `Backlog: define persistent worker supervision boundary`

- URL: https://github.com/Alive24/shea-symphony/issues/324
- Why it exists: the OpenAI Symphony spec and Elixir reference own retry timers,
  active-run reconciliation, process-exit classification, continuation checks,
  and stall restart at the orchestrator layer. Shea has partial bounded-loop and
  status pieces, but not a dedicated Backlog seed for the persistent worker
  supervision boundary.
- Dream confidence: Medium.
