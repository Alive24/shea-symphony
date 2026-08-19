# Final No-Claim Readiness

Run this phase only after confirmed writes have targeted readback or when the
operator requested a read-only readiness audit.

## Verify

Use the pinned repository surfaces to check:

- workflow and configuration parsing;
- resolved resource-manifest schema, enabled groups, dependency closure, and
  exact Markdown sources;
- capability contract, adapter role, and compatibility resolution;
- target repository and GitHub Project binding;
- selected harness and project-local Skill visibility;
- stable App package/release-manifest identity, installed discovery digest,
  live runtime executable identity, target, and compatibility;
- required runtime-profile validity, source drift, direct probes, and ignored
  machine-local path;
- configured baseline formatting, lint, build, test, or documentation checks;
- every confirmed file and external Project readback.

Do not weaken or omit configured baseline verification to obtain a green
result. Classify each check as `ready`, `not ready`, `skipped with reason`, or
`blocked`, and preserve exact safe diagnostics.

## Prove No Claim

Use read-only planning, validation, profiles, Doctor, tracker, and Project
surfaces only. Do not invoke Main/Review/Merge `once`, `loop`, `claim`, state
transition, or issue-creation commands. Read back relevant lane-claim fields and
issue state when an external Project is bound, and report any pre-existing
claim separately from setup activity.

## Report

Return one setup report with:

- target repository/root;
- stable release tag and full immutable commit;
- selected harnesses and vendored Skills;
- stable App asset, install/reuse outcome, discovery identity, and unsigned
  platform-security boundary;
- resolved core and optional resource groups plus exact installed sources;
- added, unchanged, kept-conflict, replaced, and manually merged paths;
- runtime-profile identity and safe readiness summary;
- external Project actions and readback;
- verification commands/results;
- remaining conflicts, credentials, permissions, or operator decisions;
- overall `ready` or `not ready` conclusion;
- explicit confirmation that setup created no issue, claimed no issue, changed
  no lane state, and launched no Main, Review, or Merge agent.

UAT remains operator-owned. A ready setup report proves configuration and
execution readiness, not acceptance of an implementation issue.
