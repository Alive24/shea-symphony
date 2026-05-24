# Dream Topic: CLI And Skill Drift

## Theme

Repo-owned skill instructions and supervised tmux lane startup need their own
drift checks. Recent dogfood showed two cases where the workflow did the right
thing only after the operator noticed the mismatch manually.

## Evidence Anchors

- `skills/shea-symphony/suite/shea-symphony-issue-forge-dream/SKILL.md` lists
  source-window examples using `cargo run -- project-state`,
  `cargo run -- inspect`, and older nearby command shapes.
- A live Dream run attempted `cargo run -- inspect workflows/shea-symphony.md`
  and received `unexpected argument 'workflows/shea-symphony.md'`; the current
  usable command is `cargo run -- project inspect workflows/shea-symphony.md
  '#<issue>'`, while the broad status surface is `cargo run -- debug
  workflows/shea-symphony.md`.
- `docs/dream-log/2026-05-18-01-issue-295-dream-skill-rehearsal/RUN.md`
  recorded older command names in the source inventory, proving the stale
  examples can propagate into Dream Logs.
- The #298 merge-lane tmux log showed Codex at an `External agent config
  detected` migration prompt after workspace trust, and the event log recorded
  prompt injection stopped because the trust prompt could not be cleared.

## Existing Coverage Checked

- #284 already handled grouped CLI topology. This topic does not reopen command
  design.
- #315 covers source/local/Gemini/session skill readiness, but not validation
  that runnable examples inside repo-owned skills still match the CLI.
- #314 covers richer `forge create` output.
- #305, #312, and #318 cover retry storms and long-running command visibility,
  but not the specific Codex first-run/config-migration prompt.
- #306 covers Gemini review workspace access, not Codex tmux prompt state.
- `docs/cli-command-reference.md` already documents current `project inspect`
  and `debug` forms, so canonical docs exist; the gap is drift prevention.

## Candidate Triage

### Codex Config-Migration Prompt Handling

- Backlog seed: #319
- Dream confidence: Medium
- Promotion path: Issue Forge should decide between read-only preflight,
  clearer fail-closed classification, or an explicit opt-in auto-response.

### Repo-Owned Skill Command Example Validation

- Backlog seed: #320
- Dream confidence: Medium
- Promotion path: Issue Forge should decide whether the first slice is a lint,
  Doctor warning, release checklist, or fixture-backed command-snippet test.

## Coverage Decision

These candidates are related but not duplicates. #319 is runtime startup
interaction with Codex in tmux. #320 is static repo-owned skill/doc command
example drift. #315 may later provide a readiness matrix that points at skill
files, but it does not validate whether runnable examples are still executable.

## Lane-Authority Note

This topic log is advisory only. Main, Review, Merge, and Doctor should not
treat it as an invariant unless a candidate is later promoted into an issue
contract or a repo-owned doc/skill/CLI check.
