# Installable Shea Resource Contract

`.shea/resources.v1.json` is the small source declaration for the repository-
owned payload consumed by global `setup-shea`. It is not a target lockfile,
package database, upgrade ledger, or overwrite authority.

The `core` group is always enabled and contains the target-owned operational
Skills, workflow/capability contracts, lane and backend prompts, runtime
templates, and authoritative contract documentation. Global `setup-shea` is
deliberately absent. Optional groups are explicit:

- `deepen` installs the report-only `shea-deepen` extension;
- `halo_research` installs the HALO research seed extension;
- `parent_subissues` installs parent topology/readiness resources;
- `shea_docs` reserves the future documentation extension and is unavailable
  until that separately scoped work lands.

The runtime resolves the core group plus configured optional dependencies,
rejects cycles and unavailable or escaping resources, validates every selected
file/directory, and reports exact Markdown sources. Workflow templates are
validated from the enabled closure rather than a fixed all-template bundle.
Selecting an optional group adds its complete dependency closure; omitting it
does not make its resources a readiness requirement.

Setup expands directory entries to exact staged files, uses the standard Skills
CLI for copied Skill directories, and reconciles all remaining manifest paths
by byte. Target repositories own installed files and may customize them. A
differing target file remains an explicit conflict; the manifest never grants
silent replacement or deletion authority.
