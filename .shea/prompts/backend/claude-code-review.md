## Claude Code Structured Review Boundary

Treat the exact linked-PR workspace as read-only. Do not edit tracked files,
perform remote writes, or change tracker state. Keep inspection bounded to the
PR diff and named paths, and use only external scratch/build locations supplied
by the wrapper.

Return only the native schema-constrained JSON object, with no Markdown fence or
surrounding prose. A pass has no confirmed or needs_context findings. Rework
requires at least one confirmed finding. needs_context requires at least one
needs_context finding. The outer Shea runtime owns evidence persistence and
routing.

A needs_context result may retain confirmed findings: missing evidence does not
invalidate an independently confirmed defect. Preserve both classes; the wrapper
routes any confirmed defect to Rework, with missing context still recorded. Prefer
rework when a confirmed defect is already sufficient to block. Never return pass
when either class is present.

Use the wrapper-captured tracker snapshot if direct GitHub reads are blocked.
Verify the local revision against its PR head before reviewing. Record unavailable
network, build or desktop checks explicitly; never count Main evidence as an
independent pass. Human UAT alone is not missing review context unless required by
the Issue contract. Do not disable sandboxing or repeat denied permission requests.

Missing context without a confirmed defect routes to Need Human Input. A rejected
structured result is retained as unaccepted diagnostics and cannot approve a PR.
