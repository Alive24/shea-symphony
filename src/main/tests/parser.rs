use super::*;

#[path = "parser/forge.rs"]
mod forge;
#[path = "parser/lanes.rs"]
mod lanes;

#[test]
fn parses_grouped_autopilot_plan_json_command() {
    let Command::AutopilotPlan {
        workflow_path,
        json,
    } = parse(&["autopilot", "plan", "workflows/jade-symphony.md", "--json"])
    else {
        panic!("expected autopilot plan command");
    };

    assert_eq!(workflow_path, PathBuf::from("workflows/jade-symphony.md"));
    assert!(json);
}

#[test]
fn parses_grouped_autopilot_loop_flags() {
    let Command::AutopilotLoop { options } = parse(&[
        "autopilot",
        "loop",
        "workflows/jade-symphony.md",
        "--max-iterations",
        "3",
        "--write",
        "--main-max-concurrent",
        "2",
        "--review-max-concurrent",
        "4",
        "--merge-max-concurrent",
        "1",
        "--poll-interval-ms",
        "250",
        "--json",
    ]) else {
        panic!("expected autopilot loop command");
    };

    assert_eq!(
        options.workflow_path,
        PathBuf::from("workflows/jade-symphony.md")
    );
    assert_eq!(options.max_iterations, Some(3));
    assert!(options.write);
    assert!(options.recover);
    assert_eq!(options.main_max_concurrent, Some(2));
    assert_eq!(options.review_max_concurrent, Some(4));
    assert_eq!(options.merge_max_concurrent, Some(1));
    assert_eq!(options.poll_interval_ms, Some(250));
    assert!(options.json);
}

#[test]
fn rejects_unbounded_autopilot_loop_for_now() {
    assert!(Command::parse(vec![
        "autopilot".into(),
        "loop".into(),
        "workflows/jade-symphony.md".into(),
    ])
    .is_err());
}

#[test]
fn autopilot_loop_help_documents_foreground_boundary() {
    let help = help_text(&["autopilot", "loop", "--help"]);

    assert!(help.contains("bounded foreground CLI supervisor"));
    assert!(help.contains("Bounded number of foreground autopilot iterations"));
    assert!(help.contains("Preview the bounded all-lane tick without mutation"));
}

#[test]
fn clap_parser_preserves_default_plan_compatibility() {
    assert_eq!(
        parse(&[]),
        Command::Plan {
            workflow_path: PathBuf::from("WORKFLOW.md"),
            json: false,
        }
    );
    assert_eq!(
        parse(&["examples/dry-run-workflow.md"]),
        Command::Plan {
            workflow_path: PathBuf::from("examples/dry-run-workflow.md"),
            json: false,
        }
    );
}

#[test]
fn clap_parser_keeps_operator_command_aliases() {
    assert!(Command::parse(vec!["status".into(), "examples/dry-run-workflow.md".into()]).is_err());
    assert_eq!(
        parse(&["validate-workflow", "examples/dry-run-workflow.md"]),
        Command::Validate {
            workflow_path: PathBuf::from("examples/dry-run-workflow.md")
        }
    );
    assert_eq!(
        parse(&["audit-project", "examples/dry-run-workflow.md"]),
        Command::Doctor {
            options: DoctorOptions {
                workflow_path: Some(PathBuf::from("examples/dry-run-workflow.md")),
                json: false,
                strict: false,
                display: DisplayMode::Plain,
                interactive: false,
                auto_fix: false,
                write: false,
                stale_after_ms: 10_800_000,
                action: None,
            }
        }
    );
    assert_eq!(
        parse(&["profiles", "examples/dry-run-workflow.md"]),
        Command::Profiles {
            workflow_path: PathBuf::from("examples/dry-run-workflow.md")
        }
    );
    assert_eq!(
        parse(&["debug", "examples/dry-run-workflow.md"]),
        Command::Debug {
            workflow_path: PathBuf::from("examples/dry-run-workflow.md")
        }
    );
}

#[test]
fn parses_inspect_state_filters() {
    assert_eq!(
        parse(&[
            "project",
            "inspect",
            "examples/github-project-workflow.md",
            "#284",
            "--lane",
            "main"
        ]),
        Command::ProjectInspect {
            workflow_path: PathBuf::from("examples/github-project-workflow.md"),
            issue_ref: "#284".into(),
            lane: Some(AgentSessionLaneArg::Main),
        }
    );
}

#[test]
fn parses_project_state_read_surface() {
    assert_eq!(
        parse(&["project", "state", "examples/github-project-workflow.md"]),
        Command::ProjectState {
            options: ProjectStateOptions {
                workflow_path: PathBuf::from("examples/github-project-workflow.md"),
                display: DisplayMode::Plain,
            }
        }
    );
}

#[test]
fn parses_project_state_tui_display() {
    assert_eq!(
        parse(&[
            "project",
            "state",
            "examples/github-project-workflow.md",
            "--display",
            "tui"
        ]),
        Command::ProjectState {
            options: ProjectStateOptions {
                workflow_path: PathBuf::from("examples/github-project-workflow.md"),
                display: DisplayMode::Tui,
            }
        }
    );
}

#[test]
fn parses_status_json_flag() {
    assert_eq!(
        parse(&["status", "show", "examples/dry-run-workflow.md", "--json"]),
        Command::Plan {
            workflow_path: PathBuf::from("examples/dry-run-workflow.md"),
            json: true,
        }
    );
}

#[test]
fn parses_skills_status_readiness_command() {
    assert_eq!(
        parse(&[
            "skills",
            "status",
            "workflows/jade-symphony.md",
            "--suite-path",
            "skills/jade-symphony/suite",
            "--session-skills",
            "jade-symphony-doctor,jade-symphony-manual-main",
            "--require-gemini",
            "--json",
        ]),
        Command::SkillsStatus {
            input: SkillStatusInput {
                workflow_path: PathBuf::from("workflows/jade-symphony.md"),
                suite_path: Some(PathBuf::from("skills/jade-symphony/suite")),
                codex_dir: None,
                gemini_dir: None,
                require_gemini: true,
                session_skills: vec!["jade-symphony-doctor,jade-symphony-manual-main".into()],
                session_skills_file: None,
            },
            json: true,
        }
    );
}

#[test]
fn parses_doctor_repair_human_review_command() {
    assert_eq!(
        parse(&[
            "doctor-repair-human-review",
            "examples/github-project-workflow.md",
            "--dry-run"
        ]),
        Command::DoctorRepairHumanReview {
            workflow_path: PathBuf::from("examples/github-project-workflow.md"),
            write: false
        }
    );
    assert_eq!(
        parse(&[
            "doctor-repair-human-review",
            "examples/github-project-workflow.md",
            "--write"
        ]),
        Command::DoctorRepairHumanReview {
            workflow_path: PathBuf::from("examples/github-project-workflow.md"),
            write: true
        }
    );
}

#[test]
fn parses_doctor_json_and_strict_flags() {
    assert_eq!(
        parse(&[
            "doctor",
            "examples/github-project-workflow.md",
            "--json",
            "--strict"
        ]),
        Command::Doctor {
            options: DoctorOptions {
                workflow_path: Some(PathBuf::from("examples/github-project-workflow.md")),
                json: true,
                strict: true,
                display: DisplayMode::Plain,
                interactive: false,
                auto_fix: false,
                write: false,
                stale_after_ms: 10_800_000,
                action: None,
            }
        }
    );
}

#[test]
fn parses_short_doctor_commands() {
    assert_eq!(
        parse(&["doctor", "--interactive"]),
        Command::Doctor {
            options: DoctorOptions {
                workflow_path: None,
                json: false,
                strict: false,
                display: DisplayMode::Plain,
                interactive: true,
                auto_fix: false,
                write: false,
                stale_after_ms: 10_800_000,
                action: None,
            }
        }
    );
    assert_eq!(
        parse(&["doctor", "--auto-fix", "--dry-run"]),
        Command::Doctor {
            options: DoctorOptions {
                workflow_path: None,
                json: false,
                strict: false,
                display: DisplayMode::Plain,
                interactive: false,
                auto_fix: true,
                write: false,
                stale_after_ms: 10_800_000,
                action: None,
            }
        }
    );
    assert_eq!(
        parse(&["doctor", "repair", "194"]),
        Command::Doctor {
            options: DoctorOptions {
                workflow_path: None,
                json: false,
                strict: false,
                display: DisplayMode::Plain,
                interactive: false,
                auto_fix: false,
                write: false,
                stale_after_ms: 10_800_000,
                action: Some(DoctorAction::Repair(DoctorRepairIssueOptions {
                    issue_ref: "194".into(),
                    write: false,
                    move_need_human_input: false,
                    mark_pr_ready: false,
                    confirm_handoff_ready: false,
                })),
            }
        }
    );
}

#[test]
fn parses_status_api_command() {
    assert_eq!(
        parse(&[
            "status",
            "serve",
            "examples/dry-run-workflow.md",
            "--bind",
            "127.0.0.1:0",
            "--once"
        ]),
        Command::StatusApi {
            workflow_path: PathBuf::from("examples/dry-run-workflow.md"),
            bind: "127.0.0.1:0".parse().unwrap(),
            once: true,
        }
    );
}

#[test]
fn parses_agent_session_commands() {
    assert_eq!(
        parse(&[
            "session",
            "start",
            "workflows/jade-symphony.md",
            "#220",
            "--lane",
            "review",
            "--run",
            "20260517T1404Z-issue220-review-manual",
            "--write"
        ]),
        Command::SessionStart {
            workflow_path: PathBuf::from("workflows/jade-symphony.md"),
            issue_ref: "#220".into(),
            lane: AgentSessionLaneArg::Review,
            run_id: "20260517T1404Z-issue220-review-manual".into(),
            write: true,
        }
    );
    assert_eq!(
        parse(&["session", "list", "workflows/jade-symphony.md"]),
        Command::SessionList {
            workflow_path: PathBuf::from("workflows/jade-symphony.md"),
        }
    );
    assert_eq!(
        parse(&[
            "session",
            "attach",
            "workflows/jade-symphony.md",
            "jade-review-220"
        ]),
        Command::SessionAttach {
            workflow_path: PathBuf::from("workflows/jade-symphony.md"),
            session: "jade-review-220".into(),
            exec: false,
        }
    );
    assert!(Command::parse(vec!["agent-session".into(), "list".into()]).is_err());
    assert!(Command::parse(vec!["review-session".into(), "WORKFLOW.md".into()]).is_err());
    assert!(Command::parse(vec!["merge-session".into(), "WORKFLOW.md".into()]).is_err());
}

#[test]
fn parses_lane_claim_command_groups() {
    assert_eq!(
        parse(&[
            "main",
            "claim",
            "workflows/jade-symphony.md",
            "#265",
            "--worker",
            "codex-manual-main",
            "--source",
            "manual",
            "--write"
        ]),
        Command::LaneClaim {
            workflow_path: PathBuf::from("workflows/jade-symphony.md"),
            issue_ref: "#265".into(),
            lane: AgentSessionLaneArg::Main,
            worker: "codex-manual-main".into(),
            source: CliLaneClaimSource::Manual,
            write: true,
        }
    );
    assert_eq!(
        parse(&[
            "review",
            "claim",
            "workflows/jade-symphony.md",
            "#265",
            "--worker",
            "gemini-manual-review"
        ]),
        Command::LaneClaim {
            workflow_path: PathBuf::from("workflows/jade-symphony.md"),
            issue_ref: "#265".into(),
            lane: AgentSessionLaneArg::Review,
            worker: "gemini-manual-review".into(),
            source: CliLaneClaimSource::Manual,
            write: false,
        }
    );
}

#[test]
fn parses_unified_session_commands() {
    assert_eq!(
        parse(&[
            "session",
            "start",
            "workflows/jade-symphony.md",
            "#265",
            "--lane",
            "main",
            "--run",
            "20260517T0909Z-issue265-main-manual",
            "--write"
        ]),
        Command::SessionStart {
            workflow_path: PathBuf::from("workflows/jade-symphony.md"),
            issue_ref: "#265".into(),
            lane: AgentSessionLaneArg::Main,
            run_id: "20260517T0909Z-issue265-main-manual".into(),
            write: true,
        }
    );
    assert_eq!(
        parse(&["session", "list", "workflows/jade-symphony.md"]),
        Command::SessionList {
            workflow_path: PathBuf::from("workflows/jade-symphony.md"),
        }
    );
}

#[test]
fn dogfood_smoke_is_not_a_cli_entrypoint() {
    let help = help_text(&["--help"]);
    assert!(!help.contains("dogfood-smoke"));

    let error = Command::parse(vec![
        "dogfood-smoke".into(),
        "examples/github-project-workflow.md".into(),
        "--dry-run".into(),
    ])
    .unwrap_err();

    assert!(error.contains("unexpected argument 'examples/github-project-workflow.md'"));
}

#[test]
fn parses_cleanup_plan_command() {
    assert_eq!(
        parse(&["clean", "plan", "examples/github-project-workflow.md"]),
        Command::CleanPlan {
            workflow_path: PathBuf::from("examples/github-project-workflow.md")
        }
    );
    assert_eq!(
        parse(&["clean", "audit", "examples/github-project-workflow.md"]),
        Command::CleanAudit {
            workflow_path: PathBuf::from("examples/github-project-workflow.md")
        }
    );
}

#[test]
fn parses_cleanup_workspaces_command() {
    assert!(Command::parse(vec![
        "cleanup-workspaces".into(),
        "examples/github-project-workflow.md".into(),
        "--write".into()
    ])
    .is_err());
    assert!(Command::parse(vec![
        "workspace-cleanup".into(),
        "examples/github-project-workflow.md".into()
    ])
    .is_err());
}

#[test]
fn parses_workspace_discovery_commands() {
    assert_eq!(
        parse(&["workspace", "list", "workflows/jade-symphony.md"]),
        Command::WorkspaceList {
            workflow_path: PathBuf::from("workflows/jade-symphony.md")
        }
    );
    assert_eq!(
        parse(&["workspace", "show", "workflows/jade-symphony.md", "#253"]),
        Command::WorkspaceShow {
            workflow_path: PathBuf::from("workflows/jade-symphony.md"),
            issue_ref: "#253".into(),
        }
    );
    assert_eq!(
        parse(&[
            "workspace",
            "adopt",
            "workflows/jade-symphony.md",
            "#253",
            "/tmp/issue-253",
            "--write"
        ]),
        Command::WorkspaceAdopt {
            workflow_path: PathBuf::from("workflows/jade-symphony.md"),
            issue_ref: "#253".into(),
            path: PathBuf::from("/tmp/issue-253"),
            write: true,
        }
    );
    assert_eq!(
        parse(&[
            "workspace",
            "ensure",
            "workflows/jade-symphony.md",
            "#253",
            "--pr",
            "254",
            "--branch",
            "feature/issue-253-worktree-discovery",
            "--write"
        ]),
        Command::WorkspaceEnsure {
            workflow_path: PathBuf::from("workflows/jade-symphony.md"),
            issue_ref: "#253".into(),
            pr_ref: Some("254".into()),
            branch: Some("feature/issue-253-worktree-discovery".into()),
            write: true,
        }
    );
}

#[test]
fn clap_parser_treats_help_flags_as_successful_help() {
    assert!(help_text(&["--help"]).contains("Usage: jade-symphony"));
    assert!(help_text(&["-h"]).contains("Usage: jade-symphony"));
}

#[test]
fn clap_parser_preserves_subcommand_specific_help() {
    let link_pr = help_text(&["project", "link-pr", "--help"]);
    assert!(link_pr.contains("Usage: jade-symphony project link-pr"));
    assert!(link_pr.contains("<path-to-WORKFLOW.md>"));
    assert!(link_pr.contains("<ISSUE_REF>"));
    assert!(link_pr.contains("<PR_REF>"));

    let workpad = help_text(&["project", "workpad", "--help"]);
    assert!(workpad.contains("Usage: jade-symphony project workpad"));
    assert!(workpad.contains("<MARKDOWN_PATH>"));

    let set_state = help_text(&["project", "set-state", "--help"]);
    assert!(set_state.contains("Usage: jade-symphony project set-state"));
    assert!(set_state.contains("<STATE>"));

    let forge_promote = help_text(&["forge", "promote", "--help"]);
    assert!(forge_promote.contains("Usage: jade-symphony forge promote"));
    assert!(forge_promote.contains("--operator-confirmation"));
    assert!(forge_promote.contains("--readback-summary"));
}

#[test]
fn workspace_help_explains_discovery_and_adoption_boundaries() {
    let workspace = help_text(&["workspace", "--help"]);
    assert!(workspace.contains("Discover and record per-issue git worktrees"));
    assert!(workspace.contains("safe local-worktree coordination surface"));
    assert!(workspace.contains("never runs `gh pr checkout`"));

    let list = help_text(&["workspace", "list", "--help"]);
    assert!(list.contains("read-only Project-wide inventory"));
    assert!(list.contains("orphan-looking worktrees"));

    let show = help_text(&["workspace", "show", "--help"]);
    assert!(show.contains("read-only preflight for Review and Merge agents"));
    assert!(show.contains("Multiple strong candidates require operator choice"));

    let adopt = help_text(&["workspace", "adopt", "--help"]);
    assert!(adopt.contains("operator-selected existing worktree"));
    assert!(adopt.contains("It does not create a worktree"));
    assert!(adopt.contains("--write"));

    let ensure = help_text(&["workspace", "ensure", "--help"]);
    assert!(ensure.contains("reuse"));
    assert!(ensure.contains("workflow-configured workspace root"));
    assert!(ensure.contains("never runs `gh pr checkout`"));
    assert!(ensure.contains("Workspace Evidence"));
}

#[test]
fn clap_parser_preserves_write_intent_for_mutating_commands() {
    assert_eq!(
        parse(&[
            "project",
            "set-state",
            "examples/github-project-workflow.md",
            "#4",
            "agent_review",
            "--write"
        ]),
        Command::SetState {
            workflow_path: PathBuf::from("examples/github-project-workflow.md"),
            issue_ref: "#4".into(),
            state: "agent_review".into(),
            write: true
        }
    );
}

#[test]
fn clap_parser_preserves_review_outcome_mapping() {
    assert_eq!(
        parse(&[
            "review",
            "fake",
            "examples/github-project-workflow.md",
            "#4",
            "--outcome",
            "confirmed",
            "--write"
        ]),
        Command::ReviewFake {
            workflow_path: PathBuf::from("examples/github-project-workflow.md"),
            issue_ref: "#4".into(),
            outcome: FakeReviewOutcome::ConfirmedFinding,
            write: true
        }
    );
}

#[test]
fn parses_project_issue_read_surface() {
    assert_eq!(
        parse(&[
            "project",
            "issue",
            "examples/github-project-workflow.md",
            "#235",
            "--json"
        ]),
        Command::ProjectIssue {
            workflow_path: PathBuf::from("examples/github-project-workflow.md"),
            issue_ref: "#235".into(),
            json: true
        }
    );
}

#[test]
fn parses_project_timeline_comment_write_surface() {
    assert_eq!(
        parse(&[
            "project",
            "timeline-comment",
            "examples/github-project-workflow.md",
            "#235",
            "/tmp/human-review-note.md",
            "--write"
        ]),
        Command::TimelineComment {
            workflow_path: PathBuf::from("examples/github-project-workflow.md"),
            issue_ref: "#235".into(),
            markdown_path: PathBuf::from("/tmp/human-review-note.md"),
            write: true,
        }
    );
}

#[test]
fn parses_manual_review_authority_commands() {
    assert_eq!(
        parse(&[
            "review",
            "claim",
            "examples/github-project-workflow.md",
            "#235",
            "--worker",
            "Gemini A",
            "--write"
        ]),
        Command::LaneClaim {
            workflow_path: PathBuf::from("examples/github-project-workflow.md"),
            issue_ref: "#235".into(),
            lane: AgentSessionLaneArg::Review,
            worker: "Gemini A".into(),
            source: CliLaneClaimSource::Manual,
            write: true
        }
    );

    assert!(Command::parse(vec![
        "review-clear-claim".into(),
        "examples/github-project-workflow.md".into(),
        "#235".into(),
        "--write".into()
    ])
    .is_err());
}

#[test]
fn review_group_help_hides_legacy_flat_review_commands() {
    let help = help_text(&["--help"]);

    assert!(help.contains("review"));
    assert!(!help.contains("review-claim"));
    assert!(!help.contains("review-pass"));
    assert!(!help.contains("review-reject"));
    assert!(!help.contains("review-clear-claim"));
}

#[test]
fn parses_grouped_review_commands() {
    let command = Command::parse(vec![
        "review".into(),
        "loop".into(),
        "examples/review-fixture-workflow.md".into(),
        "--once".into(),
        "--fake-outcome".into(),
        "confirmed".into(),
    ])
    .unwrap();

    let Command::ReviewLoop { options } = command else {
        panic!("expected grouped review loop command");
    };
    assert_eq!(
        options.workflow_path,
        PathBuf::from("examples/review-fixture-workflow.md")
    );
    assert!(options.once);
    assert_eq!(
        options.fake_outcome,
        Some(FakeReviewOutcome::ConfirmedFinding)
    );
}

#[test]
fn parses_review_freshness_command() {
    let command = Command::parse(vec![
        "review".into(),
        "freshness".into(),
        "--issue".into(),
        "#33".into(),
        "--prior-head".into(),
        "old-head".into(),
        "--current-head".into(),
        "new-head".into(),
        "--prior-base".into(),
        "old-base".into(),
        "--current-base".into(),
        "new-base".into(),
        "--changed-file".into(),
        "docs/dogfood-readiness.md".into(),
        "--stale-reason".into(),
        "merge-conflict".into(),
        "--rework-class".into(),
        "mechanical-conflict-resolution".into(),
        "--patch-summary".into(),
        "Resolved conflict without semantic changes.".into(),
    ])
    .unwrap();

    let Command::ReviewFreshness { input } = command else {
        panic!("expected review-freshness command");
    };

    assert_eq!(input.issue_ref, "#33");
    assert_eq!(input.changed_files, vec!["docs/dogfood-readiness.md"]);
    assert_eq!(input.stale_reason, ReviewStaleReason::MergeConflict);
    assert_eq!(
        input.rework_class,
        ReviewReworkClass::MechanicalConflictResolution
    );
    assert!(input.patch_summary.unwrap().contains("Resolved conflict"));
}
