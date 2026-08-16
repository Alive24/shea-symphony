# GitHub Failure Before Write

## Observed input

- Latest-release lookup, tag-to-commit resolution, or a selected resource fetch
  fails before confirmation.

## Expected plan

- No complete immutable source set exists, so no write plan is confirmable.

## Expected result

- Discard incomplete staging, report an actionable retry, and leave repository,
  machine, GitHub Project, issue state, and lane claims unchanged.
