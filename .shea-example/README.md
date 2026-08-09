# Shea Target Runtime Baseline

This directory is the committed baseline for a target repository. Initialize a
local runtime with:

```bash
shea-symphony target-runtime init /path/to/target-repo
```

The generated `.shea/` directory is local runtime state and should stay out of
commits.

After initialization, use `shea-symphony-runtime-onboarding` to inspect the
repository and propose `.shea/runtime-profile.json`. The skill requires
operator confirmation before writing and never installs tools. When the target
is ready to fail closed on missing or stale tool evidence, set
`runtime_profile.required: true` in its local workflow.
