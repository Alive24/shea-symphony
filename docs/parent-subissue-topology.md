# Parent/Subissue Integration Branch Topology

This document defines how Jade Symphony models a parent issue with native
GitHub subissues when the work is too large for the normal one issue, one
branch, one pull request flow.

The normal single-issue flow remains the default. Use this topology only when a
human has intentionally promoted a parent tracking issue and concrete native
GitHub subissues under it.

## Source Of Truth

GitHub native sub-issue links are the source of truth for parent/subissue
hierarchy.

Issue body text, workpad notes, Project fields, branch names, and PR bodies may
record execution details, but they must not become a competing hierarchy source.
If supplemental metadata disagrees with native GitHub sub-issue links, the
native relationship wins and the supplemental evidence needs repair.

## Terms

- Parent issue: the native GitHub parent tracking issue. It owns the final
  Human Review unit and the final merge path into `main`.
- Subissue: a native GitHub subissue of the parent. It owns a bounded slice of
  implementation and review evidence.
- Parent integration branch: the shared branch where accepted subissue PRs land
  before the parent's final PR targets `main`.
- Subissue branch: the per-subissue feature branch created by the normal Main
  Agent lane.
- Parent final PR: the pull request from the parent integration branch to
  `main` after all native subissues are Done and merged into the parent branch.

## Branch Topology

Parent integration branches use:

```text
integration/issue-<parent-number>-<short-slug>
```

For parent issue `#243`, the expected integration branch is:

```text
integration/issue-243-parent-subissue-orchestration
```

The parent integration branch is owned by the parent issue. It is not a normal
subissue branch and should not carry unrelated work. It exists to collect only
accepted subissue work before the parent final PR targets `main`.

Subissue branches keep the existing Main Agent naming convention:

```text
feature/issue-<subissue-number>-<short-slug>
```

## PR Target Rules

Subissue PRs target the parent integration branch by default, not `main`.

The parent final PR targets `main` and uses the parent integration branch as its
head. This keeps each subissue reviewable on its own while preventing partial
parent work from landing on `main` before the parent is complete.

Exceptions are allowed only when a human explicitly records that a subissue is
not part of the parent integration branch. The exception must be visible in the
issue workpad and PR body, and later doctor checks should treat missing
exception evidence as unsafe.

## Supplemental Metadata

Record the following execution metadata in durable evidence:

| Metadata | Preferred location | Purpose |
| --- | --- | --- |
| Parent integration branch | Parent issue body or parent workpad | Identifies the shared branch owned by the parent issue. |
| Subissue branch | Subissue workpad and PR head branch | Identifies the normal per-issue implementation branch. |
| Subissue PR target | Subissue PR base branch and subissue workpad | Proves the PR targets the parent integration branch. |
| Parent final PR target | Parent issue workpad and parent PR base/head | Proves the parent branch is proposed for `main`. |
| Review evidence | Subissue and parent workpads | Preserves Main, Agent Review, Human Review, and merge decisions. |
| Merge evidence | Subissue and parent workpads | Shows subissue PRs merged into the parent branch and parent PR status. |

These fields supplement the native hierarchy. They do not define it.

## Subissue Done Semantics

A subissue may move to `Done` after all of these are true:

- its implementation PR has passed the normal Main and Review path;
- its PR has merged into the parent integration branch;
- the subissue workpad records the PR, target branch, review evidence, and merge
  evidence;
- the subissue remains a native child of the parent issue or has an explicit
  recorded exception.

Subissue `Done` means the slice has landed in the parent branch. It does not mean
the parent work is approved for `main`.

## Parent Human Review Gate

The parent issue remains the final Human Review unit.

The parent must not move to `Human Review` until all of these are true:

- every native GitHub subissue declared under the parent is `Done`;
- every subissue PR intended for the parent branch is merged into that parent
  integration branch;
- the parent final PR exists from the parent integration branch to `main`;
- the parent workpad records the subissue set, parent branch, final PR, and any
  explicit exceptions;
- independent Review Agent evidence exists for the parent final PR.

The Main Agent still stops at `Agent Review`. The Review Agent and human
approval boundaries do not change for parent/subissue work.

## Unsafe Topologies

Later doctor checks should flag these examples:

- a native subissue PR targets `main` without an explicit exception;
- a subissue claims parent membership only through body text or workpad notes,
  without a native GitHub sub-issue relationship;
- a subissue is marked `Done` before its PR merges into the parent integration
  branch;
- a parent issue enters `Human Review` before all native subissues are `Done`;
- a parent final PR targets `main` while the parent branch is missing one or more
  subissue merge records;
- branch evidence points at a different parent issue than the native GitHub
  relationship.

Issue #273 should turn these into doctor invariants. Issue #274 should teach
lane flows how to use the parent integration branch during live execution.

## Doctor Diagnostics

`doctor` is diagnostic-only for parent/subissue topology. It reads GitHub native
parent and subissue links as hierarchy authority, then uses parent issue body or
workpad branch evidence, linked PR base/merge state, and branch-name hints as
supplemental execution evidence.

Blocker findings cover unsafe execution states:

- native subissue PRs targeting `main` instead of the parent integration branch;
- missing or ambiguous parent integration branch evidence on a native parent;
- `Done` subissues without linked PR or workpad evidence showing merge into the
  parent integration branch;
- parent issues in `Human Review` before every native subissue is `Done` and
  merged into the parent integration branch.

Warning findings cover repairable metadata inconsistencies, such as body-only
parent membership or a subissue PR target that disagrees with the parent branch
without directly targeting `main`. Doctor does not repair these states, retarget
PRs, edit native GitHub relationships, move Project statuses, or replace the
#274 lane-flow work.

## Dry Fixture Verification

The credential-free topology fixture lives at:

```text
examples/fixtures/parent-subissue-topology.json
```

Run its focused verification with:

```bash
cargo test --test parent_subissue_topology
```

That test validates the happy path, unsafe topology examples, and the
documentation boundaries above without changing live lane behavior.
