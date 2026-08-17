# Guarded Promotion

Use `issue.promote` only to replace one existing Backlog seed in place with a
complete executable contract. Validate the current title/body at the Todo gate,
prepare the Promotion Note evidence and any native relationships, and obtain
confirmation bound to those exact bytes and effects.

Execute the guarded promotion once. Its status change to Todo is the final
mutation. Read back issue content, relationships, claim fields, and Project
state; never approximate promotion with raw issue edits or a separate state
change.
