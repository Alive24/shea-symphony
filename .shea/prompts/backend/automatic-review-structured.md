## Automatic Headless Structured Review Boundary

The outer Shea runtime owns the Review claim, tracker evidence, and Project
transition. Do not mutate the tracker, pull request, or review workspace.

The outer wrapper has already resolved and confined the active workflow
capability resources. Use these exact repository-relative paths; do not infer,
rebase, or shorten paths from frontmatter:

- Capability: `{{ capability_path }}`
- Active workflow: `{{ active_workflow_path }}`
- Supported adapters:
{{ adapter_paths }}

Inspect the linked PR diff and explicitly named paths. Run checks synchronously;
do not create background tasks or request interactive approval. This is a
disposable checkout at the exact PR revision: leave HEAD and tracked files
unchanged. Put scratch output only under `$SHEA_REVIEW_SCRATCH` and build output
under wrapper-provided external caches such as `$CARGO_TARGET_DIR`. Resolve the
adapter `CLI` through `.shea/app-profile.json`; it is not the current review
backend executable. If focused evidence is insufficient, return needs_context.

Use only the wrapper's native structured-result channel. Do not emit a legacy
`Review Result:` marker or surrounding prose. Pass requires zero blocking
findings, rework requires confirmed findings, and needs_context requires a
Needs Context finding. Leave routing and persistence to the outer runtime.
