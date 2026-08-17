# Scope And Evidence

Bind the repository root and read every applicable instruction before scanning.
Record the operator's named area, question, or pain point as the scope boundary.

## Select the window

- If the operator names files, a module, subsystem, workflow, or change pain,
  use that area and do not infer another one.
- Otherwise inspect at most the latest 40 first-parent commits and identify at
  most 20 repeatedly changed first-party files. Let those hot spots define one
  bounded area; widen only when changes are too scattered to support a scope.
- Exclude generated output, vendored dependencies, archived Dream Logs, local
  reports, build artifacts, and unrelated documentation history.

State the selected scope and why it is active before reading implementation
details. Ask one short scope question only when repository identity or the
operator's named area is genuinely ambiguous.

## Gather evidence

Read the scoped code, its callers, focused tests, applicable instructions, and
relevant authoritative docs or ADRs. `CONTEXT.md` may inform terminology when
present but is never required. Use repository-owned names rather than imposing
a glossary or generic architecture vocabulary on domain concepts.

Explore organically and note concrete friction:

- understanding one behavior requires bouncing among several files;
- callers repeat ordering, validation, error, or configuration knowledge;
- tests bypass the public interface or duplicate integration setup;
- coupled changes recur across the same files; or
- a nominal abstraction passes complexity through instead of containing it.

Keep path and symbol anchors for each observation. Do not infer a candidate
from file size, style preference, missing docs, or theoretical purity alone.
Carry no more than three evidenced candidates into the architecture lens.
