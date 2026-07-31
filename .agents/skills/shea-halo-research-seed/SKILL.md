---
name: shea-halo-research-seed
description: Interactively turn a rough agent-loop, harness, or observability concern into a neutral, evidence-bound Shea Halo Research Issue seed. Use when a user wants help framing, reviewing, creating, or starting a Halo research item; referencing an earlier Halo run without biasing the new lifecycle; deciding whether a seed is ready for `Halo Research`; or learning how the Project status triggers Halo.
---

# Shea Halo Research Seed

Guide the operator from an uncertain improvement question to an approved Issue
seed. Seed the investigation; do not perform Halo's research or prescribe its
conclusion.

## Establish the contract

Resolve these facts before any tracker write:

- observed repository and checkout;
- target `.shea/halo.toml` or complete local replacement;
- configured GitHub Project, status field, and `Halo Research` state;
- default branch and current remote revision;
- available runtime experiment and verification actions;
- related Issues, snapshots, traces, PRs, and blockers;
- whether the Shea Halo worker can read the Project and target checkout.

Read the target's current configuration and the repository's
`examples/halo-research-issue.md` when available. Treat configured workflow
actions and Project status as authoritative. Do not guess a repository, Project,
fixture, revision, or command.

Explain the current control boundary when useful:

- the operator or another automation creates the seed Issue;
- placing its Project item in `Halo Research` starts Halo;
- Halo appends research to that same Issue;
- Halo alone may route verified work to `Todo` or `Done`;
- a human must not pre-authorize the result or force `Todo`.

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

## Separate context from authority

Use prior Issues heavily as a map, not as a conclusion:

- link the useful workpad decisions, reports, and blockers;
- summarize what the old lifecycle suggested;
- name its exact revision or validity boundary when known;
- mark historical traces and snapshots as context-only unless they are valid
  inputs under the new lifecycle contract;
- require fresh evidence when the base, candidate, configuration, or runtime
  capability has changed.

Never copy raw workpad JSON, traces, prompts, model/tool payloads, credentials,
endpoints, or host paths into a seed.

## Draft the seed

Use the smallest complete shape:

```markdown
# Halo Research: <observable improvement question>

## Research objective

<Outcome to understand or improve, without prescribing the answer.>

## Historical context

<Optional prior findings, their references, and their validity boundary.>

## Questions to resolve

- <Current evidence question>
- <Competing hypothesis>
- <Smallest falsifying experiment>
- <Trace sufficiency or framework-native integration question>

## Available evidence and capabilities

- <Current source, configured experiment, fixture, or trusted reference>

## Guardrails

- <Safety, locality, privacy, authority, and non-duplication boundaries>

## Completion contract

- Record facts, hypotheses, experiments, and residual risks in append-only Halo
  workpad comments.
- Bind experiments, candidate traces, HALO analysis, and deterministic
  verification to the exact candidate revision.
- Prefer the current framework-native Inference guide and record its provenance.
- Keep raw evidence local and publish only bounded, sanitized receipts.
- Treat any Halo branch as `experimental_snapshot_only`.
- Move this same item to `Todo` only for a fully gated implementation handoff.
- Use `Done` for an evidence-supported no-change result.
- Use `Need Human Input` only for a verified external blocker with a required
  operator action.
```

Omit `Historical context` or `Available evidence and capabilities` only when
genuinely inapplicable. Add target-specific evidence requirements and
verification commands when they make the research falsifiable.

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

## Create and start safely

Show the complete draft and list assumptions, dependencies, and readiness.
Obtain explicit confirmation before creation. Restate the exact Issue,
repository, Project, and target status before a write.

Use the configured workflow or connected provider to:

1. quality-gate and create the Issue;
2. add it to the configured Project if needed;
3. record structured blockers or relationships;
4. read the Issue and Project item back.

Treat creation and start as separate mutations unless the user explicitly
approves both. Starting means changing the Project item's status to
`Halo Research`; explain that the worker will claim it on its next poll.

After starting, report the Issue URL, number, pinned/freshness assumptions,
Project status, worker prerequisites, and what evidence the operator should
expect next. Do not manually move the item to `Todo`, write Halo's result, reuse
its experimental branch for implementation, or close a predecessor without a
separately confirmed append-only disposition note.

## Interaction style

Keep questions short and conversational. Prefer a recommended phrasing over a
form dump. During drafting, show what remains unresolved. At confirmation,
provide the full seed, readiness classification, exact next mutation, and any
reason not to start yet.
