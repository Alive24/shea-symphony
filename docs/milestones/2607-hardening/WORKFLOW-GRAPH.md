# Workflow Graph

Status: Draft

## Purpose

The Workflow Graph is the intended long-term model for workflow structure. It
should eventually make the workflow easy to inspect, resume, disable, extend,
and visualize.

For 2607 Hardening, the goal is narrower: clarify the current workflow shape so
future graph work is possible without breaking the MVP. Do not implement a full
graph runtime in this milestone.

## 2607 Scope

2607 should:

- keep the existing workflow configuration working;
- preserve Main, Review, Merge, Doctor, App reads, and current operator flows;
- define vocabulary for states, standard nodes, extension nodes, edges, and
  extension output;
- organize the current workflow around Tracker State first;
- allow standard behavior to be configured;
- allow extension behavior to be inserted;
- avoid forcing every runtime step through a new graph executor.

2607 should not:

- replace the current workflow engine with a graph runtime;
- require all existing workflow config to migrate at once;
- build a graph editor;
- define a complete extension module loading system.

Workflow Graph execution and a full extension module system belong to
`2608 Workflow Graph Extension`.

## Configuration Location

Supported:

- `WORKFLOW.md`
- `.shea/workflow.md`

Preferred:

- `.shea/workflow.md`

Markdown is preferred because it can carry YAML front matter, prose, prompt
templates, and workflow documentation in one tracked file.

The machine-readable portion must be independently parseable. The Markdown body
may explain operator policy, prompts, and workflow rationale, but hard runtime
behavior must not depend on unstructured prose.

## Compatibility Bias

Avoid shifting too far from the current workflow configuration during
hardening. The first shape can be hybrid:

- Tracker State is the top-level organization layer;
- standard Symphony behavior is configurable;
- extension nodes or hooks can be inserted around standard behavior;
- future graph nodes and edges can be derived from this structure.

This preserves the current workflow while moving toward an explicit graph.

## Model

Future target:

```text
WorkflowGraph
  nodes:
    - id
    - kind
    - state
    - runner
    - write_policy
    - llm_policy
  edges:
    - from
    - to
    - condition
    - transition
```

2607 can document and validate this model without using it as the only runtime
execution source.

## Node Kinds

- `standard`: implemented by Symphony.
- `extension`: configured by the workflow and executed under Symphony policy.

Standard nodes are not replaced in place. To customize behavior, disable the
standard node and add extension nodes around or instead of that path.

For 2607, hooks may remain as a compatibility mechanism if they are clearly
attached to a Tracker State or standard node. The later graph milestone can
convert those hooks into first-class extension nodes.

## Standard States

- `Backlog`
- `Todo`
- `Need to Clarify`
- `In Progress`
- `Need Human Input`
- `Agent Review`
- `Human Review`
- `Merging`
- `Rework`
- `Done`

## Example Shape

```text
Backlog -> Todo
Todo -> Need to Clarify
Todo -> In Progress
In Progress -> Agent Review
Agent Review -> Rework
Agent Review -> Human Review
Human Review -> Merging
Human Review -> Rework
Merging -> Done
Merging -> Need Human Input
Rework -> In Progress
```

In 2608, extension nodes can be inserted as ordinary graph nodes:

```text
In Progress
  -> semantic_contract_gate
  -> independent_agent_review
  -> attention_budget_gate
  -> Human Review
```

Extension nodes may affect the next graph edge or recommend entry into a core
node. If that recommendation requires tracker state to change, the write still
goes through the Symphony transition path.

## Edge Conditions

Start with fixed enum conditions:

- `ready`
- `passed`
- `failed`
- `blocked`
- `needs_clarification`
- `needs_human_input`
- `approved`
- `rejected`
- `merged`
- `terminal`

Avoid arbitrary expression languages until enum conditions fail in real use.

## Extension Node Output

Extension output should use a fixed schema. First draft:

```text
ExtensionResult
  decision
  evidence
  proposed_transition
  proposed_next_node
  questions
  blocked_reason
```

Open question: whether this schema should be Markdown-first with front matter,
pure JSON, or both.

## Side Effect Policy

Do not over-design side effect policy in 2607.

The hard rule is:

- tracker writes always go through Symphony transitions.

The first practical extension policy can be much smaller:

- workspace writes allowed or not allowed;
- transition requests allowed or not allowed.

External service calls should be handled by the runner/tool policy already in
use. Do not introduce a separate broad side-effect taxonomy in 2607. Revisit it
in 2608 when extension modules become first-class.

## LLM Policy

LLM use is allowed in extension nodes. LLM output is evidence or proposal until
Symphony validates and applies it.

## Visualization

Future App target:

- read-only graph;
- highlighted current node;
- current issue state;
- latest transition evidence;
- disabled/bypassed nodes visible.

For 2607, a state-grouped read-only view is enough. It can show the current
Tracker State, standard behavior, inserted hooks/extensions, and transition
evidence without requiring a full graph runtime.
