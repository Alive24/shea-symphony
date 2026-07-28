# Parking Lot

Status: Living notes

Loose ideas that are not ready to become backlog notes.

## Ideas

- Decide whether extension node output should be Markdown with YAML front
  matter, pure JSON, or both.
- Decide first concrete timing budget for Project snapshot reads.
- Decide App graph visualization minimum shape.
- Build a subtraction inventory: repeated tracker reads, direct tracker writes,
  lane-local state mapping, App source-of-truth inference, vendored runtime
  assumptions, CLI shape drift, and files that are large because they mix
  ownership boundaries.
- Decide the first snapshot shape that combines tracker state and local runtime
  state for App dashboard refresh without eager artifact reads.

## Post-2607 Shea/OpenWiki Integration

Evaluate a Shea extension module that makes repository documentation maintenance
easier without absorbing OpenWiki into the Symphony runtime.

The Shea-owned UX may:

- show whether OpenWiki is present and read its checked-in
  `openwiki/.last-update.json` freshness metadata;
- guide repository-specific instructions and exclusions;
- explicitly trigger a local OpenWiki update and present the resulting diff for
  human review;
- scaffold, inspect, enable, or disable a provider-specific CI update workflow;
- explain missing GitHub secret/configuration prerequisites without reading or
  storing secret values;
- link generated gaps to their owning Issue, package, milestone, or ADR.

If GitHub Actions automation is enabled for a repository, the recommended
profile is:

- `workflow_dispatch` plus a low-frequency schedule, not a pull-request event
  that evaluates untrusted changes with the inference secret;
- a pinned OpenWiki version and pinned action revisions;
- one repository-scoped OpenAI project service-account key supplied only
  through GitHub Actions secrets;
- least-privilege job permissions, a timeout, and a concurrency group;
- a documentation-only pull request limited to `openwiki/**`, `AGENTS.md`, and
  `CLAUDE.md`;
- required human review and no automatic merge.

Shea may render this profile, validate checked-in configuration, and link to
the provider's secret/settings page. It must not claim a secret exists by
reading its value, proxy inference credentials through the App, or become the
scheduler. The CI provider remains the execution owner.

The boundary must keep OpenWiki responsible for content generation and its
format, GitHub Actions or another CI provider responsible for scheduling, and
the inference provider responsible for credentials and usage accounting. Shea
must not reimplement the OpenWiki agent, store an OpenAI API key in repository
or App state, silently upload repository content, automatically merge generated
documentation, or make documentation refresh part of Temporal issue
orchestration.

Tracking: future Shea UX/extension milestone after 2607 Hardening; no Issue or
ADR is assigned yet. Revisit only after the core App/Tauri operator boundary in
T2607-07 is stable.
