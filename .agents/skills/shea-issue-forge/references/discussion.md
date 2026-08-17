# Focused Discussion

Ask one to three focused questions per turn and continue only while ambiguity
would materially change execution. Offer a recommended answer when operator
intent already implies one, allow skipped discussion with recorded assumptions,
and keep deferred ideas outside the accepted scope.

Resolve the goal, why now, target package, in/out of scope, guardrails,
dependencies, trusted references, code-state freshness, verification, and
operator-owned UAT. Check native parent/subissue topology when independently
testable slices, multiple lanes, or review risk would otherwise create one
oversized PR.

For a batch, the parent owns final Human Review and UAT. Ordinary children own
implementation plus Agent Review and normally route to Merging. Record a
Subissue Human Review Exception only when a child genuinely needs one.

Never create dispatchable work whose blocker exists only in prose. Add the
native blocker through the selected adapter in the same guarded workflow, or
keep the issue non-executable until the blocker is terminal.
