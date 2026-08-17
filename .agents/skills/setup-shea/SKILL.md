---
name: setup-shea
description: Onboard or reconcile Shea Symphony in a target repository from one immutable stable GitHub release. Use for initial project-local setup, incomplete setup, environment drift, operator-requested reconciliation, or runtime-profile and repository-contract problems routed back from Doctor.
---

# Setup Shea

Provide the single operator-facing setup path for repository contracts, normal
Skills, harness visibility, GitHub Project binding, and machine-local runtime
readiness. Keep discovery and reconciliation agent-led.

## Authority

Read repositories, installed tools, GitHub release metadata, and Project
configuration without confirmation. Before any repository, machine, or
external Project mutation, show one exact plan and obtain confirmation bound to
that plan. Re-prepare when paths, bytes, source revision, or external effects
change.

Never claim an issue, start Main/Review/Merge, or mutate lane state. Do not
install tools, change shell startup, store credentials, or overwrite an
existing differing file without an explicit operator decision.

## Run Setup

1. Read [target-discovery.md](references/target-discovery.md) to bind the target,
   inspect applicable instructions, inventory current Shea files, and detect
   supported harnesses.
2. Read [immutable-release.md](references/immutable-release.md) and resolve the
   latest stable Shea release once to both its tag and full commit before
   planning any remote resource.
3. Read [resource-manifest.md](references/resource-manifest.md) to load the
   versioned source manifest, select the complete core group, and resolve any
   explicitly requested optional extension closure.
4. Read [workflow-project.md](references/workflow-project.md) when binding a
   workflow, capability adapter, App profile, or GitHub Project.
5. Read [reconciliation.md](references/reconciliation.md) to plan selected
   Skills and repository-owned Markdown, classify additions/unchanged files/
   conflicts, confirm exact effects, and apply safe writes.
6. Read [runtime-profile.md](references/runtime-profile.md) when runtime
   requirements are missing, stale, drifted, or explicitly requested.
7. Read [readiness.md](references/readiness.md) for readback, verification, and
   the final no-claim readiness report.

## Source and Ownership Invariants

- Use only the pinned commit for remote reads after release resolution; never
  fetch mutable `main` or re-resolve "latest" inside the confirmed run.
- Use the standard Skills CLI's project-local `--copy` installation in a
  temporary staging project for Skill vendoring, then reconcile those copied
  files into the target. The repository source manifest declares the selected
  resource closure; it is not target state, a package database, or an
  upstream-hash registry.
- Do not bundle target Skills, workflows, prompts, templates, or workpads under
  this Skill. Fetch chosen canonical paths from the pinned revision.
- Treat vendored and generated target files as operator-owned. A byte-identical
  file is unchanged; any existing differing file is a conflict until the
  operator chooses keep, replace, or a reviewed manual merge.
- Stage all remote inputs before confirmation. A GitHub or validation failure
  before writes must leave the target and external Project unchanged.

Finish with an explainable readiness result, remaining conflicts or blockers,
the stable tag and immutable commit, and an explicit statement that no issue
was claimed and no agent lane was launched.
