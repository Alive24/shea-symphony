# One Immutable Revision Per Run

## Observed input

- Latest stable release resolves to tag `v1.2.3`.
- The tag resolves through an annotated tag object to full commit
  `2222222222222222222222222222222222222222`.

## Expected plan

- Fetch every Skill and Markdown resource from the full commit.
- Use the tag only as display evidence and do not query latest again.
- Reject any later resource URL containing `main`, a different tag, or another
  commit.

## Expected result

- The final report records one tag and one full commit for every remote input.
