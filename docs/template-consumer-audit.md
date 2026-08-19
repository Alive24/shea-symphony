# Markdown Template Consumer Audit

The taxonomy separates the canonical Main workpad, append-only operational
evidence, Human Review decisions, and read-only reports. Runtime keys remain
typed; paths are repository-owned Markdown selected through the resource
closure.

| Template path | Typed/runtime consumer | Lifecycle reason |
| --- | --- | --- |
| `template/issue/executable.md` | `src/issue_templates.rs`, `src/issue_forge.rs`, `src/commands/gate.rs` | Single executable-Issue layout and same-file semantic-intent owner |
| `template/workpad/main-handoff.md` | `src/lanes/main_loop/handoff.rs` | Canonical stable Main sections |
| `template/workpad/main-handoff-failure.md` | `src/lanes/main_loop/handoff.rs` | Stable Main recovery receipt |
| `template/workpad/main-assignee-ownership.md` | `src/lanes/main_loop/handoff.rs` | Pre-claim ownership receipt |
| `template/workpad/main-quality-gate.md` | `src/commands/gate.rs` | Pre-dispatch contract receipt |
| `template/workpad/main-runtime-ownership.md` | `src/lanes/main_loop/handoff.rs` | Stable run identity section |
| `template/workpad/main-usage-limit-pause.md` | `src/lanes/main_loop/handoff.rs` | Resumable Main recovery state |
| `template/evidence/agent-review.md` | `src/review.rs` | Automatic append-only Review run |
| `template/evidence/agent-review-handoff.md` | `src/handoff.rs` | Main-to-Review invariant evidence |
| `template/evidence/repeated-review-failure.md` | `src/review.rs` | Compact same-cause retry evidence |
| `template/evidence/manual-review.md` | `src/lanes/review/manual.rs` | Manual independent Review record |
| `template/evidence/review-invalid-handoff.md` | `src/lanes/review/automatic.rs` | Review refusal before backend run |
| `template/evidence/rework-diagnostic.md` | `src/rework.rs` | Typed Review/Main Rework trigger |
| `template/evidence/review-freshness.md` | `src/review/freshness.rs` | Post-change review validity report |
| `template/evidence/doctor-triage.md` | `src/doctor/report.rs` | General Doctor finding/repair receipt |
| `template/evidence/human-review-repair.md` | `src/doctor/report.rs` | Narrow invalid-Human-Review rollback receipt |
| `template/evidence/merge-run.md` | `src/merge_lane.rs` | Complete merge attempt and readback |
| `template/evidence/merge-repair.md` | `src/merge_lane.rs` | Repair-only evidence before routing |
| `template/evidence/forge-rework-run.md` | `src/commands/forge/rework.rs` | Confirmed contract replacement evidence |
| `template/evidence/forge-rework-blocked.md` | `src/commands/forge/rework.rs` | No-write blocker evidence |
| `template/evidence/lane-session.md` | `src/commands/session/start.rs` | Backend/session identity evidence |
| `template/evidence/workspace-adoption.md` | `src/commands/workspace.rs` | Operator-selected workspace singleton |
| `template/evidence/workspace-ensure.md` | `src/commands/workspace/ensure.rs` | Runtime-created/reused workspace validation |
| `template/evidence/parent-topology.md` | `src/commands/forge.rs` | Optional native parent branch evidence |
| `template/decision/human-review.md` | `.agents/skills/shea-human-review/SKILL.md` | Operator-confirmed append-only decision |
| `template/report/parent-batch-readiness-report.md` | `.agents/skills/shea-human-review/SKILL.md` | Optional read-only advisory report |

The audit deleted orphaned `template/workpad/rework-run.md`; typed
`rework-diagnostic` is the active trigger record. The pairs above remain
distinct where required fields or lifecycle authority differ: Doctor repair is
narrower than general triage, blocked Forge Rework must prove no replacement,
manual Review records a human actor/claim while automatic Review records a job
ledger, Merge repair precedes routing while Merge run includes the final
readback, and adoption versus ensure has different workspace authority.

The six Main receipts remain separate small sources because the section-aware
canonical workpad merger updates their stable sections independently. Combining
them into one conditional template would obscure accepted lifecycle boundaries.

The executable-Issue source is deliberately separate from workpads: Forge
renders its visible layout, while the optional semantic model gate receives the
trusted raw source alongside an explicitly untrusted candidate and deterministic
facts. Production Rust does not duplicate its headings, labels, or rubric.
