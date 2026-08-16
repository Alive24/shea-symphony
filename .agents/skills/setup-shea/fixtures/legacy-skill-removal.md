# Confirmed Legacy Skill Removal

## Observed input

- A target contains `shea-symphony-issue-forge-dream` and
  `shea-symphony-issue-forge-reflect` directories.
- Reflect is Git-tracked and modified; Dream contains one bounded untracked
  customized Markdown file.

## Expected plan

- Classify each exact directory as `remove_legacy` only after enumerating its
  paths, sizes, digests, recoverability, focused tracked diff, and complete
  current customized text.
- Show the preimage tree digest and require explicit confirmation bound to each
  deletion. Install no compatibility alias or deprecated wrapper.
- Stop on drift, unreadable/binary/large content, symlinks, nested repositories,
  or path escape.

## Expected result

- After confirmation, delete only the two exact legacy directories and read
  back their absence.
- Without confirmation, preserve both directories and report setup not fully
  reconciled.
