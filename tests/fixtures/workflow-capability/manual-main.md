---
kind: shea-workflow-capability-consumer-fixture
fixture_version: 1
consumer: manual-main
capability: .shea/contracts/workflow-capability.v1.md
adapter: .shea/contracts/adapters/legacy-cli.v1.md
required_reads:
  - workflow.resolve
  - issue.read
  - issue.inspect
  - evidence.read
  - pull_request.read
  - relationships.read
guarded_actions:
  - workspace.adopt
  - lane.claim
  - workpad.upsert
  - issue.transition
  - pull_request.link
---

# Manual Main Consumer

The capability contract owns mutation ordering and uncertainty handling. The
Manual Main policy supplies narrower authority: one eligible operator-selected
issue, one canonical workspace and branch, and a stop at Agent Review.

1. Resolve the active workflow and adapter, then perform the targeted issue,
   evidence, relationship, and workspace reads required by Main policy.
2. Prepare workspace adoption and claim only after the issue's selection gates
   pass; use the operator-selected Main invocation as confirmation for those
   exact issue-scoped actions.
3. Record the plan, implement accepted scope, and keep verification evidence in
   the canonical workpad.
4. Prepare and execute the ready pull-request linkage only after verification,
   then require exact linkage readback.
5. Make the Agent Review transition the last mutation and stop after targeted
   issue readback.

This fixture intentionally contains no adapter command syntax and does not
change the live Manual Main skill.
