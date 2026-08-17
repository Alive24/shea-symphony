# Guarded Creation

Use `issue.create` only for a prepared body that passes the gate appropriate to
its requested initial state. A Todo issue must be fully executable and assigned;
a Backlog seed may use the bounded seed gate and remains non-executable.

Prepare the exact title, body, assignee, status, blocker/subissue relationships,
and Project effect. Obtain confirmation, execute once, then read back the
returned issue, relationships, and state. Issue creation is the final mutation.
Report the URL, number, Project status, assumptions, and any integration gap.
