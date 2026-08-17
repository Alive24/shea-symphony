# Report And Retention

Produce one local visual artifact without making repository-visible changes.

## Safe path

1. Resolve the Git root and require `.shea/local/` to be ignored by Git. If it
   is not ignored, stop; do not edit `.gitignore` automatically.
2. Use `.shea/local/deepen/<run-id>/`, where `run-id` is a generated UTC
   timestamp plus a short safe slug, never an unchecked path from user input.
3. Create `.shea-deepen-run.json` before the report with schema version, run
   id, repository identity, selected scope, and creation time. This is the
   cleanup marker; include no credentials or ambient environment.
4. Write only `report.html` beside the marker. Confirm `git status --short`
   shows no new tracked or untracked repository-visible path.

## Report contract

Keep `report.html` at or below 500 KiB. Use semantic HTML, inline CSS, and
optional inline SVG only. Do not use remote scripts, remote fonts, Tailwind,
Mermaid, CDNs, network URLs, protocol-relative URLs, external stylesheets, or
linked assets.

Show the repository, scope, evidence window, and result. Include zero to three
candidate cards. Each card contains:

- involved files and symbols;
- observed comprehension or change friction;
- proposed deepening direction without a designed interface;
- expected locality, leverage, and testing benefit;
- a concise side-by-side before/after visualization; and
- recommendation strength.

Name at most one top recommendation. For a no-finding run, show what was
examined and why every suspected candidate failed the lens instead of padding
the report.

Validate candidate count, one-or-zero top recommendation, byte size, marker,
ignored path, and absence of remote dependencies before presenting the absolute
path. Do not open a GUI unless the operator asks.

## Bounded retention

Retain at most five completed run directories. Before a sixth run, list only
run directory names and marker metadata; never load historical report contents.
Prepare the exact oldest marked directory for deletion and obtain operator
confirmation before cleanup. If confirmation is declined, do not create a new
run.

Delete only a direct child of the resolved `.shea/local/deepen/` root when it
is not a symlink, contains a valid matching `.shea-deepen-run.json`, and has
the expected `report.html`. Stop on unmarked, malformed, oversized, nested, or
path-escaping content. Never delete the root, the current run, an unresolved
path, or any repository-visible file.
