# 2608 Workflow Graph Extension

Status: Future Draft

## Purpose

2608 Workflow Graph Extension is the follow-up milestone for turning the
state-grouped workflow structure from 2607 Hardening into a first-class
Workflow Graph and extension module system.

2607 prepares the boundaries. 2608 can make the graph executable.

## Relationship To 2607

2607 Hardening should:

- preserve current workflow behavior;
- clarify Symphony and Shea boundaries;
- centralize tracker transitions;
- define snapshots and state evidence;
- organize existing workflow behavior around Tracker State;
- define insertion points for hooks/extensions.

2608 should build on that by:

- making graph nodes and edges first-class runtime objects;
- mapping state-grouped workflow steps to graph nodes;
- loading extension modules through a defined contract;
- validating graph configuration;
- exposing graph structure to App visualization;
- allowing extensions to influence graph direction through structured output.

## Candidate Scope

- `.shea/workflow.md` machine-readable graph schema.
- Standard node registry for Symphony-owned behavior.
- Extension module registry for Shea/project behavior.
- Graph validation for nodes, edges, states, policies, and missing runners.
- Graph execution or staged graph adoption.
- Extension output schema with structured fields and Markdown detail.
- Disabled and bypassed node semantics.
- Read-only graph visualization in the App.

## Explicit Non-Goals For 2607

The following should not be forced into 2607 Hardening:

- complete Workflow Graph runtime execution;
- full extension module loading system;
- graph editor;
- wholesale migration of existing workflow configuration;
- expression-language edge conditions;
- broad side-effect taxonomy beyond transition requests and workspace writes.

## Open Questions

- How should current state-grouped workflow steps map to graph nodes?
- Should extension output use JSON, YAML front matter plus Markdown, or both?
- How much graph execution should be adopted before replacing compatibility
  hooks?
- What does a minimal extension module package look like?
- How should graph layout be represented for App visualization?
