# Halo Intake, Challenge and Readiness

## Conduct focused intake

Ask one to three questions per round. Offer a recommended answer when the user
has already implied it, and allow the user to skip discussion with recorded
assumptions.

Resolve, in this order:

1. **Observable uncertainty** — What operator-visible behavior or agent-loop
   outcome is not understood well enough?
2. **Why now** — What decision, failure, or opportunity makes research useful?
3. **Historical context** — Which prior findings are hypotheses, which evidence
   is immutable history, and which evidence may be used for the new lifecycle?
4. **Competing explanations** — What plausible alternatives must Halo
   distinguish rather than assume away?
5. **Evidence floor** — What source, trace, experiment, HALO analysis, and
   verification would make a conclusion trustworthy?
6. **Boundaries** — What must Halo not expose, duplicate, mutate, or treat as a
   required external service?
7. **Disposition** — What observable evidence permits `Todo`, `Done`, or
   `Need Human Input` without dictating which one Halo must choose?

Keep implementation details out unless they define an experimental capability
or safety boundary. Phrase the objective as “determine whether” rather than
“prove that.”

## Challenge the draft

Before presenting it, check:

- **Neutrality:** Does the seed allow the expected finding to be disproved?
- **Single objective:** Is it one coherent research question rather than a
  backlog bundle?
- **Freshness:** Will Halo pin the intended current base after all prerequisites
  land?
- **Executable evidence:** Does the configured target own a real experiment,
  not merely a copied historical artifact or synthetic substitute?
- **Competing hypotheses:** Can the evidence distinguish at least the plausible
  alternatives?
- **Safety:** Are raw or private evidence, credentials, endpoints, and host paths
  excluded?
- **Authority:** Is Halo, rather than the operator, responsible for the final
  handoff and Project routing?
- **Readability:** Can a collaborator understand the question and completion
  contract without opening structured workpad JSON?

Repair leading, mixed, stale, unsafe, or unverifiable language before asking for
approval.

## Gate readiness

Classify the seed:

- `Ready to start`: target, configuration, experiment, fixture, permissions,
  and current base are available.
- `Ready to create, blocked from start`: the Issue can be created in `Backlog`,
  but a structured dependency must become terminal before `Halo Research`.
- `Need clarification`: an ambiguity changes the objective, authority, evidence
  contract, or safety boundary.
- `Blocked`: tracker reads, configuration, repository identity, or required
  capability cannot be verified.

Never place an item in `Halo Research` while a required experiment, fixture,
permission, or structured dependency is unavailable.
