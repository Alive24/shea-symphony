# Dream Run: CLI And Skill Drift Mining

Date: 2026-05-18
Run: `2026-05-18-02-dream-cli-skill-drift`
Mode: write-mode Dream continuation
Operator context: sustained Dream run over recent Project state, Doctor warnings, dogfood logs, review-loop evidence, and repo-owned skills

## Source Inventory

- `git status --short --branch`
- `git fetch origin`
- `git merge --ff-only origin/main`
- `cargo run -- project state workflows/shea-symphony.md`
- `cargo run -- doctor workflows/shea-symphony.md`
- `cargo run -- debug workflows/shea-symphony.md`
- `cargo run -- project inspect workflows/shea-symphony.md '#243'`
- `cargo run -- project issue workflows/shea-symphony.md '#319' --json`
- `cargo run -- project issue workflows/shea-symphony.md '#320' --json`
- Open Backlog readback for #305, #306, #307, #308, #316, #317, #318, #319, and #320.
- Todo/readiness readback for #314 and #315.
- `gh issue list --repo Alive24/shea-symphony --state open --json number,title,url --limit 80`
- `docs/dream-log/INDEX.md`
- `docs/dream-log/2026-05-18-01-issue-295-dream-skill-rehearsal/RUN.md`
- `docs/dream-log/2026-05-18-01-issue-295-dream-skill-rehearsal/topic-runtime-recovery.md`
- `skills/shea-symphony/suite/shea-symphony-issue-forge-dream/SKILL.md`
- `skills/shea-symphony/suite/`
- Recent event log and tmux evidence under `~/.shea-symphony/artifacts/Alive24/shea-symphony/default/logs/`.

## Round Summary

The source window showed that most recent pain had already been captured as
Backlog or Todo work:

- review-loop backend retry storms are covered by #305 and #312;
- Gemini workspace visibility is covered by #306;
- automatic review-loop registry evidence is covered by #307;
- artifact namespace confusion is covered by #308;
- Human Review batch handoff is covered by #316;
- dirty PR merge preflight planning is covered by #317;
- long-running command silence is covered by #318;
- malformed Main Agent claim repair is covered by #299;
- per-repo skill install/session visibility is covered by #315;
- richer `forge create` output is covered by #314.

Two concrete gaps remained worth seeding:

- Codex tmux lane startup can pass workspace trust and then stop on an
  `External agent config detected` migration prompt. This is not the same as
  Gemini workspace access, retry policy, or generic long-running UX.
- Repo-owned skill command examples can drift from the current grouped CLI
  topology. The Dream skill itself still listed obsolete source-window command
  shapes, and a live run of the top-level `inspect` example failed.

## Created Backlog

- #319 `Backlog: handle Codex config-migration prompts in tmux lanes`
- #320 `Backlog: validate repo-owned skill command examples`

## Watchlist / Not Created

- Local skill install drift and missing local skills: not created because #256
  implemented Doctor diagnostics, #242 owns install/update, and #315 now covers
  the broader readiness matrix.
- `forge create` returning only a node id: not created because #314 is already
  `Todo` for issue number, URL, and Project status output.
- Long quiet Project reads and live command waits: not created because #318 is
  already open and matches the current dogfood pain.
- Review-loop quota/retry behavior: not created because #305 and #312 already
  cover retry storms and health-aware retry policy.
- #243 terminal Review Agent claim missing registry evidence: not created as a
  new seed because #307 is already the likely promotion target for automatic
  review-loop registry evidence. #243 itself remains the immediate Project item
  to inspect when ready.

## Doctor / Project Warnings

- `doctor` reports warning-only health: no blockers.
- Current Project summary after this run: `Agent Review:1, Backlog:10, Done:77, Todo:4`.
- The prominent Doctor issue warning is #243
  `terminal_lane_claim_missing_registry` for a terminal Review Agent claim with
  no matching runtime/session registry evidence.
- Local Codex/Gemini skill install warnings remain present: missing Dream,
  Doctor, Manual Review/Merge/Main skills in some roots, stale suite metadata,
  and drift from repo-owned suite copies.
- Integration gaps remain: Project v2 PR linking is still comment/autolink
  based, and live Project writes still go through `gh api graphql` under
  Shea Symphony CLI `--write` commands.

## Gemini Review Status

Pending at log creation. See `gemini-review.md` for the final lightweight review
or an explicit unavailable reason.

## Slept Enough

Slept enough: no.

Reason: this round found and created the strongest CLI/skill drift seeds, but a
later Dream round should mine the remaining `Todo`/`Agent Review` surface after
#314 and #315 move. The next useful theme is "skill readiness and Dream/Doctor
install drift after #315", because that issue may absorb several current Doctor
warnings and reduce duplicate Backlog pressure.

## Safety Notes

Dream-created issues stayed in `Backlog`. None were promoted to Todo, claimed by
Main/Review/Merge, or treated as lane authority. Project reads and issue
creation went through the Shea Symphony CLI where Project state or mutation was
involved.
