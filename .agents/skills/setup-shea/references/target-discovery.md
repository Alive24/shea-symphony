# Target Discovery And Harness Selection

Use this phase for read-only binding and inventory. Do not infer the target from
the Shea source repository when the operator named another repository.

## Bind The Target

1. Resolve the repository root with `git rev-parse --show-toplevel` and record
   its `owner/repository` identity from the target's Git remotes or confirmed
   operator input.
2. Read every `AGENTS.md` that governs the root and any path likely to change.
3. Inventory existing `.agents/skills`, `.shea` workflows, capability
   contracts, adapters, prompts, templates, workpads, App profiles, and the
   runtime profile. Record missing, present, and unreadable paths.
4. Prefer `.shea/app-profile.local.json` over `.shea/app-profile.json` only when
   resolving the current workflow and CLI. Preserve existing profile semantics;
   setup does not redesign shared-versus-local App profiles.
5. Inspect repository manifests, lockfiles, toolchain files, CI, contributor
   docs, and configured verification without changing them.

Stop when repository identity, applicable instructions, or the writable target
root is ambiguous. Ask for the smallest operator decision that resolves it.

## Detect Harnesses

Consider Codex, Claude Code, and Antigravity independently. Use read-only
evidence such as already-installed executables, application availability,
repository harness directories, and the standard Skills CLI's supported-agent
listing. Report each as `available`, `not detected`, or `unsupported by the
current Skills CLI`; do not assume all three are installed.

Let the operator select only detected/supported harnesses. Delegate project-
local Skill placement and harness-specific directory conventions to the
standard Skills CLI. Do not copy one harness's files into guessed locations or
install a harness during setup.

## Discovery Output

Record:

- target root and repository identity;
- governing instruction paths;
- current Shea workflow/CLI/profile resolution;
- existing Shea-owned and operator-customized paths;
- available harnesses and evidence;
- repository execution requirement sources;
- read failures, conflicts, and decisions still required.
