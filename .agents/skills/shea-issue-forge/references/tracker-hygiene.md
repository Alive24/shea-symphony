# Tracker And Draft Hygiene

Before claiming freshness or preparing a Forge mutation:

1. Record canonical checkout status, revision, remote, and base branch. Fetch
   the configured base. Fast-forward only a clean, strictly behind checkout;
   otherwise use the fetched ref as evidence and preserve user files.
2. Keep draft bodies only in a unique marker-bearing
   `.shea/local/forge/<run-id>/` directory after proving it is ignored. Never
   scan historical drafts into issue context.
3. On handled completion, remove only the current run directory. Later cleanup
   may remove only marker-bearing Forge runs older than 24 hours and must not
   traverse other `.shea/local/` content.
4. Record temporary-directory size and checkout status before and after. Stop
   on any unexpected repository-visible or unrelated byte change.

Resolve the repository, workflow, tracker project, default assignee, and exact
supported action before mutation. Do not assume slugs, paths, field names,
accounts, or commands. Reads may proceed without confirmation; every write uses
Prepare, Confirm, Execute, and targeted readback from the capability contract.
