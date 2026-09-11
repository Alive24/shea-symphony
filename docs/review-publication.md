# Guarded Review Dispatch and Publication

`review once WORKFLOW ISSUE` prepares one targeted Review without a claim or
backend start. `--write` additionally requires canonical checkout readiness,
Agent Review eligibility, one ready PR, the matching clean source workspace,
local dispatch exclusion and a targeted claim readback before backend launch.
Only the calling lane's explicit Issue authorization permits that write.

The per-Issue OS lock is held while the backend runs and its result is published.
It coordinates processes sharing the same logs root, including `review once`
and `review loop`. It is not a cross-host compare-and-swap operation on GitHub;
use one coordinator for hosts that do not share the local exclusion boundary.
Tracker claim readback detects observed ownership changes but is not a global
exactly-once guarantee.

## Durable boundaries

Publication receipts live under `logs_root/reviews/publications/<issue>/`.
They are written atomically and contain the Issue snapshot, claim, runtime
revision, rendered-prompt fingerprint, workspace, backend job, publication stage
and frozen evidence. The fingerprint identifies bytes; it is not a signature.

1. `Starting` is durable before requesting the tracker claim.
2. `Running` and the job ledger are durable after backend acknowledgement.
3. `ResultReady` retains the terminal job before any Review-result tracker write.
4. `EvidencePublished` follows a targeted readback of the stable run marker.
5. Terminal claim and supported non-UAT checkboxes are read back before status.
6. Status is the last mutation. Its targeted readback makes the receipt `Complete`.

Every publication checks the current Issue identity, open state, contract, claim,
PR readiness, linkage and head against the captured review. A changed head or
contract makes the result `Superseded`; it cannot approve the new revision or
replace a newer lane owner's status. The Issue body is separated from hydrated
comment evidence with `<!-- shea-symphony-attached-evidence -->`, including when
no canonical Main workpad is present. Only this run's supported PASS checkboxes
are permitted to differ during publication recovery.

`review status WORKFLOW --issue ISSUE --json` includes pending publications.
`review recover WORKFLOW ISSUE` previews a captured terminal result;
`--write` resumes publication with its original run and frozen evidence. It does
not launch a backend. A repeated completed recovery is a no-op. A comment error
is read back before retry; the same run marker prevents duplication when server
acceptance preceded a network failure.

A starting/running receipt without a captured terminal result remains pending
for Doctor process/artifact triage. A crash between OS process launch and backend
acknowledgement cannot prove that no process started. Do not delete the receipt,
clear a claim or infer an accepted verdict from raw JSON to permit another run.
An unfinished receipt prevents the next automatic dispatch. Historical runs
without receipts remain readable; this command does not fabricate missing state
for them. GitHub cannot atomically compare the entire snapshot with a mutation;
re-reads narrow races but a concurrent edit after the final read still requires
normal tracker reconciliation.

## Result interpretation

| Validated result | Routing |
| --- | --- |
| PASS, no confirmed or missing-context findings | Human Review or Merging according to the existing Issue policy. |
| Any confirmed defect, including mixed confirmed/missing-context output | Rework, retaining both classes. |
| Missing context with no confirmed defect | Need Human Input. |
| Rejected output, timeout or unavailable backend | Existing blocked/Need Human Input policy; never PASS. |

Rejected structured output remains unaccepted. Public evidence includes the
validation error and a bounded, escaped diagnostic excerpt when a raw artifact
exists. The raw artifact remains the complete local record; diagnostics cannot
be promoted to an accepted verdict. Historical `InconclusiveNeedsRework` ledgers
continue to deserialize, but new context-only results do not use that outcome.

## Validation and deployment

Run the Review parser/backend fixtures, Legacy publication tests and complete
library/Legacy suite. The publication tests exercise comment failures, errors
after server acceptance, same-run recovery after claim/checklist completion,
local exclusion, contract/head/state/claim changes and source changes during
publication. The opt-in Claude transport fixture covers clean and seeded-defect
inputs without touching a real Issue or PR. It is transport evidence, not an
independent acceptance of this source change or of a product PR.

Ship the guarded command mapping with a verified runtime that exposes
`review recover --help`. Preserve old ledgers, artifacts and the prior executable.
Do not downgrade a newer local repair to an incompatible stable App release or
claim stable App onboarding from a development binary.
