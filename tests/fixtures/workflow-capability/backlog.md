---
kind: shea-workflow-capability-consumer-fixture
fixture_version: 1
consumer: backlog
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
---

# Backlog Consumer

The capability contract owns mutation ordering and uncertainty handling.
Backlog policy remains advisory until the operator confirms one exact Backlog
creation prepared from current evidence.

1. Resolve the active workflow and adapter, then use targeted issue, evidence,
   pull-request, and relationship reads for the selected checkpoint window.
2. Keep observations and candidate drafts read-only while discussing scope,
   duplicates, blockers, staleness, and residual value.
3. Prepare one validated Backlog create effect and show it to the operator.
4. Execute only the explicitly confirmed creation, perform targeted issue and
   relationship readback, and preserve uncertainty instead of guessing.
5. Route a selected candidate to Issue Forge; do not promote or execute it.

This fixture intentionally contains no adapter command syntax and does not
change the live Backlog skill.
