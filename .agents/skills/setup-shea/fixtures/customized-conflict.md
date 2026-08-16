# Customized Local File And Newer Upstream

## Observed input

- A vendored Skill differs from the staged stable-release version.
- No managed upstream baseline or hash registry exists.

## Expected plan

- Classify the file as a conflict, show the focused difference, and default to
  `conflict_keep`.
- Offer `conflict_replace` or `conflict_manual_merge` only as explicit operator
  decisions bound to displayed bytes.

## Expected result

- Never overwrite the customized file merely because upstream is newer.
