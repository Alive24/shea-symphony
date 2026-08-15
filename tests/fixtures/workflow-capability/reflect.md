---
kind: shea-workflow-capability-consumer-fixture
fixture_version: 1
consumer: reflect
capability: .shea/contracts/workflow-capability.v1.md
adapter: .shea/contracts/adapters/legacy-cli.v1.md
required_reads:
  - workflow.resolve
  - issue.read
  - evidence.read
  - pull_request.read
  - relationships.read
guarded_actions:
  - issue.create
  - issue.promote
---

# Reflect Consumer

The capability contract owns mutation ordering and uncertainty handling.
Reflect policy remains advisory until the operator confirms one exact issue
creation or promotion prepared from current evidence.

1. Resolve the active workflow and adapter, then use targeted issue, evidence,
   pull-request, and relationship reads for the selected reflection window.
2. Keep observations and candidate drafts read-only while discussing scope,
   duplicates, dependencies, and intended status.
3. Prepare one validated create or promotion effect and show it to the operator.
4. Execute only the explicitly confirmed effect, perform targeted issue and
   relationship readback, and preserve uncertainty instead of guessing.

This fixture intentionally contains no adapter command syntax and does not
change the live Reflect skill.
