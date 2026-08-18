# Immutable Stable Release Source

Resolve one remote source identity before reading or planning repository
resources. Release lookup is read-only.

## Resolve Once

1. Query GitHub's latest-release endpoint for `Alive24/shea-symphony` and
   record the release id, `tag_name`, publication time, draft flag, and
   prerelease flag.
2. Reject a missing release, draft, prerelease, malformed tag, or repository
   mismatch. Do not fall back to `main`.
3. Resolve `refs/tags/<tag>` through GitHub Git objects. Follow annotated tag
   objects until the object type is `commit`; reject cycles, unsupported object
   types, or an abbreviated SHA.
4. Record the stable tag for operator display and the full 40-character commit
   as the only resource revision for this run.
5. Do not query "latest" again unless the operator abandons the current plan
   and explicitly starts a new setup run.

Require the resolved Release to expose the complete stable App asset set. Use
[app-runtime.md](app-runtime.md) for native package and embedded-runtime
selection; repository resources still come only from the peeled commit below.

The release tag is human-readable evidence; the commit is the fetch boundary.
Use commit-pinned GitHub content/raw-content URLs or a verified detached Git
checkout for every subsequent resource. Never substitute a tag or branch after
the commit is known.

## Stage Before Planning Writes

- Create a temporary staging directory outside the target repository.
- Initialize a checkout without fetching a default branch, fetch only
  `refs/tags/<tag>`, peel the fetched object to a commit, require it to equal the
  resolved full commit, and check out that commit detached. Abort if the tag
  moved or the checkout cannot prove the exact commit.
- Fetch every selected path at the pinned commit, fail on redirects to another
  revision, and reject missing, empty, binary, or unexpectedly large Markdown
  resources.
- Record a per-run digest of staged bytes for the displayed plan and readback.
  Do not persist an upstream-hash registry in the target.
- Validate references and selected workflow paths from the staged content
  before offering any write.

If GitHub, authentication, rate limits, tag resolution, or any resource fetch
fails, discard the incomplete staging area, report an actionable retry, and
leave the target repository and external Project unchanged. Never continue
from a partially fetched plan.
