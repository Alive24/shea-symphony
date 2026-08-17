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
