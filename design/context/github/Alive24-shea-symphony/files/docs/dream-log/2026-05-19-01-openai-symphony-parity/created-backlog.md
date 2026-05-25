# Created Backlog

## #321 `Backlog: shape Codex app-server continuation parity`

- URL: https://github.com/Alive24/shea-symphony/issues/321
- Why it exists: OpenAI Symphony treats Codex app-server continuation,
  `max_turns`, token/rate-limit accounting, and runtime status updates as a
  core orchestration loop. Shea has partial event-normalizer and subprocess
  pieces, but not the full transport/continuation/runtime wiring.
- Dream confidence: Medium.

## #322 `Backlog: define tracker-scoped dynamic tool execution`

- URL: https://github.com/Alive24/shea-symphony/issues/322
- Why it exists: the Elixir reference can execute `linear_graphql` as a dynamic
  tool, while Shea currently describes planned tools without execution or
  app-server protocol wiring. The GitHub Project tracker boundary needs an
  explicit design before implementation.
- Dream confidence: Medium.

## #323 `Backlog: wire last-known-good workflow reload into runtimes`

- URL: https://github.com/Alive24/shea-symphony/issues/323
- Why it exists: the spec requires invalid reloads to preserve the last known
  good workflow; Shea has helper semantics but not long-running runtime wiring
  or status diagnostics.
- Dream confidence: High.
