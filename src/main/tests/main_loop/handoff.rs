use super::*;
use crate::lanes::main_loop::failed_backend_can_use_live_handoff;
use crate::lanes::main_loop::run_loop_apply_recovery_workspace_report;

#[test]
fn run_loop_handoff_plan_uses_issue_workspace_and_branch_plan() {
    let config = test_config();
    let issue = tracker_issue("In Progress");

    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();

    assert_eq!(
        handoff.workspace_key,
        "issue-29-wire-runtime-state-persistence-into-main-loop"
    );
    assert!(handoff
        .workspace_path
        .ends_with("issue-29-wire-runtime-state-persistence-into-main-loop"));
    assert_eq!(
        handoff.branch_name,
        "feature/issue-29-wire-runtime-state-persistence-into-main-loop"
    );
    assert_eq!(
        handoff.pull_request.title,
        "#29: Wire runtime state persistence into main loop"
    );
    assert_eq!(handoff.pull_request.base_branch, "main");
}

#[test]
fn run_loop_handoff_plan_uses_configured_base_branch() {
    let mut config = test_config();
    config.git.base_branch = "dev-chunteng".into();
    let issue = tracker_issue("In Progress");

    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();

    assert_eq!(handoff.pull_request.base_branch, "dev-chunteng");
    assert_eq!(
        handoff.branch_target.pull_request_base_branch,
        "dev-chunteng"
    );
}

#[test]
fn run_loop_handoff_plan_rejects_branch_for_different_issue() {
    let config = test_config();
    let mut issue = tracker_issue("In Progress");
    issue.branch_name = Some("feature/issue-99-other-work".into());

    let error = run_loop_handoff_plan(&config, &issue).unwrap_err();

    assert!(matches!(
        error,
        HandoffError::BranchIssueMismatch {
            expected_issue,
            found_issue,
            ..
        } if expected_issue == "29" && found_issue == "99"
    ));
}

#[test]
fn launch_workspace_preflight_ignores_missing_historical_jade_candidate() {
    let mut config = test_config();
    let temp = tempfile::tempdir().unwrap();
    config.workspace.root = temp.path().join(".shea-symphony").join("worktrees");
    let mut issue = tracker_issue("In Progress");
    issue.linked_pull_requests.push(LinkedPullRequest {
        head_ref_name: Some(
            "feature/issue-29-wire-runtime-state-persistence-into-main-loop".into(),
        ),
        number: Some(29),
        ..Default::default()
    });
    let mut handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let planned_path = handoff.workspace_path.clone();
    let stale_path = temp
        .path()
        .join(".jade-symphony")
        .join("worktrees")
        .join("issue-29-old-main");
    let report = workspace_report(
        &issue,
        vec![workspace_candidate(
            stale_path,
            Some("feature/issue-29-wire-runtime-state-persistence-into-main-loop"),
            WorkspaceMatchStrength::Strong,
            "session_registry",
        )],
    );

    let preflight =
        run_loop_apply_launch_workspace_report(&config, &issue, &mut handoff, &report).unwrap();

    assert_eq!(handoff.workspace_path, planned_path);
    assert!(preflight.evidence.iter().any(|line| {
        line.contains("ignored_missing_workspace") && line.contains("namespace=jade-symphony")
    }));
    assert!(preflight
        .evidence
        .iter()
        .any(|line| line.contains("workspace_preflight action=prepare")));
}

#[test]
fn launch_workspace_preflight_reuses_single_clean_matching_worktree() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.workspace.root = temp.path().join("worktrees");
    std::fs::create_dir_all(&config.workspace.root).unwrap();
    let issue = tracker_issue("In Progress");
    let mut handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let worktree = config.workspace.root.join("manual-issue-29");
    init_clean_git_workspace(&worktree);
    git_ok(
        &worktree,
        &[
            "checkout",
            "-b",
            "feature/issue-29-wire-runtime-state-persistence-into-main-loop",
        ],
    );
    let report = workspace_report(
        &issue,
        vec![workspace_candidate(
            worktree.clone(),
            Some("feature/issue-29-wire-runtime-state-persistence-into-main-loop"),
            WorkspaceMatchStrength::Strong,
            "git_worktree",
        )],
    );

    let preflight =
        run_loop_apply_launch_workspace_report(&config, &issue, &mut handoff, &report).unwrap();

    assert_eq!(handoff.workspace_path, worktree);
    assert_eq!(handoff.workspace_key, "manual-issue-29");
    assert!(preflight
        .evidence
        .iter()
        .any(|line| line.contains("workspace_preflight action=reuse")));
}

#[test]
fn launch_workspace_preflight_blocks_ambiguous_live_candidates() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.workspace.root = temp.path().join("worktrees");
    std::fs::create_dir_all(&config.workspace.root).unwrap();
    let issue = tracker_issue("In Progress");
    let mut handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let first = config.workspace.root.join("issue-29-a");
    let second = config.workspace.root.join("issue-29-b");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    let report = workspace_report(
        &issue,
        vec![
            workspace_candidate(
                first,
                Some("feature/issue-29-a"),
                WorkspaceMatchStrength::Strong,
                "git_worktree",
            ),
            workspace_candidate(
                second,
                Some("feature/issue-29-b"),
                WorkspaceMatchStrength::Strong,
                "git_worktree",
            ),
        ],
    );

    let error =
        run_loop_apply_launch_workspace_report(&config, &issue, &mut handoff, &report).unwrap_err();

    assert!(matches!(
        error,
        HandoffError::WorkspacePreflightBlocked { reason, .. }
            if reason.contains("multiple strong live workspace candidates")
    ));
}

#[test]
fn launch_workspace_preflight_blocks_dirty_candidate_before_runtime() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.workspace.root = temp.path().join("worktrees");
    std::fs::create_dir_all(&config.workspace.root).unwrap();
    let issue = tracker_issue("In Progress");
    let mut handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let worktree = config.workspace.root.join("dirty-issue-29");
    init_clean_git_workspace(&worktree);
    git_ok(
        &worktree,
        &[
            "checkout",
            "-b",
            "feature/issue-29-wire-runtime-state-persistence-into-main-loop",
        ],
    );
    std::fs::write(worktree.join("scratch.txt"), "dirty").unwrap();
    let report = workspace_report(
        &issue,
        vec![workspace_candidate(
            worktree,
            Some("feature/issue-29-wire-runtime-state-persistence-into-main-loop"),
            WorkspaceMatchStrength::Strong,
            "git_worktree",
        )],
    );

    let error =
        run_loop_apply_launch_workspace_report(&config, &issue, &mut handoff, &report).unwrap_err();

    assert!(matches!(
        error,
        HandoffError::WorkspacePreflightBlocked { reason, .. }
            if reason.contains("dirty") && reason.contains("stop before app-server launch")
    ));
}

#[test]
fn recovery_workspace_preflight_reuses_dirty_matching_worktree() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.workspace.root = temp.path().join("worktrees");
    std::fs::create_dir_all(&config.workspace.root).unwrap();
    let issue = tracker_issue("In Progress");
    let mut handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let worktree = config.workspace.root.join("dirty-issue-29");
    init_clean_git_workspace(&worktree);
    git_ok(
        &worktree,
        &[
            "checkout",
            "-b",
            "feature/issue-29-wire-runtime-state-persistence-into-main-loop",
        ],
    );
    std::fs::write(worktree.join("scratch.txt"), "dirty").unwrap();
    let report = workspace_report(
        &issue,
        vec![workspace_candidate(
            worktree.clone(),
            Some("feature/issue-29-wire-runtime-state-persistence-into-main-loop"),
            WorkspaceMatchStrength::Strong,
            "runtime_state",
        )],
    );

    let preflight =
        run_loop_apply_recovery_workspace_report(&config, &issue, &mut handoff, &report).unwrap();

    assert_eq!(handoff.workspace_path, worktree);
    assert!(preflight.evidence.iter().any(|line| {
        line.contains("workspace_preflight action=reuse_dirty_recovery")
            && line.contains("scratch.txt")
            && line.contains("runtime_state")
    }));
}

#[test]
fn recovery_workspace_preflight_blocks_detached_candidate() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.workspace.root = temp.path().join("worktrees");
    std::fs::create_dir_all(&config.workspace.root).unwrap();
    let issue = tracker_issue("In Progress");
    let mut handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let worktree = config.workspace.root.join("detached-issue-29");
    init_clean_git_workspace(&worktree);
    let report = workspace_report(
        &issue,
        vec![workspace_candidate(
            worktree,
            None,
            WorkspaceMatchStrength::Strong,
            "runtime_state",
        )],
    );

    let error = run_loop_apply_recovery_workspace_report(&config, &issue, &mut handoff, &report)
        .unwrap_err();

    assert!(matches!(
        error,
        HandoffError::WorkspacePreflightBlocked { reason, .. }
            if reason.contains("detached") && reason.contains("workspace adopt")
    ));
}

#[test]
fn recovery_workspace_preflight_blocks_issue_mismatch() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.workspace.root = temp.path().join("worktrees");
    std::fs::create_dir_all(&config.workspace.root).unwrap();
    let issue = tracker_issue("In Progress");
    let mut handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let worktree = config.workspace.root.join("dirty-issue-99");
    init_clean_git_workspace(&worktree);
    git_ok(
        &worktree,
        &["checkout", "-b", "feature/issue-99-other-work"],
    );
    std::fs::write(worktree.join("scratch.txt"), "dirty").unwrap();
    let report = workspace_report(
        &issue,
        vec![workspace_candidate(
            worktree,
            Some("feature/issue-99-other-work"),
            WorkspaceMatchStrength::Strong,
            "runtime_state",
        )],
    );

    let error = run_loop_apply_recovery_workspace_report(&config, &issue, &mut handoff, &report)
        .unwrap_err();

    assert!(matches!(
        error,
        HandoffError::WorkspacePreflightBlocked { reason, .. }
            if reason.contains("does not match issue #29")
    ));
}

#[test]
fn run_loop_handoff_workpad_records_planned_pr_evidence() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let result = IssueExecutionResult {
        workspace_path: handoff.workspace_path.clone(),
        backend: "dry-run".into(),
        profile_id: None,
        instance_name: None,
        success: true,
        pending_session: false,
        session_id: Some("session-33".into()),
        run_id: None,
        backend_log_path: None,
        backend_attach_command: None,
        message: "ok".into(),
        usage_limit_pause: None,
        prompt_artifact_path: None,
        actor_role: "implementation_agent".into(),
        actor_label: "Shea Symphony Agent".into(),
        git_author: Some("Shea Symphony Agent <shea@example.invalid>".into()),
        git_identity: GitIdentityApplyResult {
            status: shea_symphony::workspace::GitIdentityApplyStatus::Applied,
            author: Some("Shea Symphony Agent <shea@example.invalid>".into()),
            applied_keys: vec!["user.name".into(), "user.email".into()],
        },
        live_handoff: Some(RunLoopLiveHandoff {
            worktree: LiveWorktreeResult {
                workspace_path: handoff.workspace_path.clone(),
                branch_name: handoff.branch_name.clone(),
                created: true,
            },
            publication: PullRequestPublication {
                branch_pushed: true,
                pr_url: "https://github.com/Alive24/shea-symphony/pull/45".into(),
                pr_created: true,
            },
            verification: "skipped:not_configured".into(),
            project_pr_link_verified: Some(true),
            pull_request_ready: Some(PullRequestReadyStatus {
                pr_url: "https://github.com/Alive24/shea-symphony/pull/45".into(),
                was_draft: false,
                marked_ready: false,
            }),
        }),
        handoff_verification: Some("skipped:not_configured".into()),
    };

    let workpad = run_loop_handoff_workpad(None, &issue, &result, &handoff, None);

    assert!(workpad.contains("### Plan"));
    assert!(workpad.contains("### Work Log"));
    assert!(workpad.contains("- [x] Read the issue contract"));
    assert!(workpad.contains("### PR / Linkage"));
    assert!(workpad.contains("Actor role: `implementation_agent`"));
    assert!(workpad.contains("Git identity: `applied:Shea Symphony Agent <shea@example.invalid>`"));
    assert!(
        workpad.contains("Workspace key: `issue-29-wire-runtime-state-persistence-into-main-loop`")
    );
    assert!(workpad
        .contains("Branch: `feature/issue-29-wire-runtime-state-persistence-into-main-loop`"));
    assert!(workpad.contains("PR title: `#29: Wire runtime state persistence into main loop`"));
    assert!(workpad.contains("Handoff verification: `skipped:not_configured`"));
    assert!(workpad.contains("Live PR: `https://github.com/Alive24/shea-symphony/pull/45`"));
}

fn workspace_report(
    issue: &TrackerIssue,
    candidates: Vec<IssueWorkspaceCandidate>,
) -> IssueWorkspaceReport {
    IssueWorkspaceReport {
        issue_ref: issue.identifier.clone(),
        title: issue.title.clone(),
        branch_hints: Vec::new(),
        candidates,
        canonical_index: None,
        warnings: Vec::new(),
    }
}

fn workspace_candidate(
    path: PathBuf,
    branch: Option<&str>,
    strength: WorkspaceMatchStrength,
    source: &str,
) -> IssueWorkspaceCandidate {
    IssueWorkspaceCandidate {
        path,
        branch: branch.map(str::to_string),
        head: Some("abc123".into()),
        strength,
        evidence: vec![WorkspaceEvidence {
            source: source.into(),
            detail: "test evidence".into(),
        }],
    }
}

#[test]
fn live_run_loop_handoff_records_pr_link_through_tracker() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let mut result = successful_live_handoff_result(&handoff);
    let adapter = RecordingAdapter::default();

    assert!(apply_live_handoff_pr_link(
        &adapter,
        &issue.identifier,
        &mut result
    ));

    assert!(result.success);
    assert_eq!(
        adapter.operations(),
        vec!["link_pr:#29:https://github.com/Alive24/shea-symphony/pull/45"]
    );
}

#[test]
fn live_run_loop_handoff_skips_link_comment_when_pr_already_visible() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let mut result = successful_live_handoff_result(&handoff);
    let adapter = RecordingAdapter::default();
    adapter
        .linked_pull_requests
        .borrow_mut()
        .push(shea_symphony::model::LinkedPullRequest {
            number: Some(45),
            url: Some("https://github.com/Alive24/shea-symphony/pull/45".into()),
            state: Some("OPEN".into()),
            is_draft: Some(false),
            source: shea_symphony::model::LinkedPullRequestSource::GithubNative,
            ..Default::default()
        });

    assert!(apply_live_handoff_pr_link(
        &adapter,
        &issue.identifier,
        &mut result
    ));

    assert!(result.success);
    assert!(adapter.operations().is_empty());
}

#[test]
fn live_run_loop_handoff_accepts_fallback_pr_evidence() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let mut result = successful_live_handoff_result(&handoff);
    let adapter = RecordingAdapter {
        confirm_link_pr: false,
        ..Default::default()
    };
    adapter
        .linked_pull_requests
        .borrow_mut()
        .push(shea_symphony::model::LinkedPullRequest {
            number: Some(45),
            url: Some("https://github.com/Alive24/shea-symphony/pull/45".into()),
            state: Some("OPEN".into()),
            is_draft: Some(false),
            source: shea_symphony::model::LinkedPullRequestSource::FallbackDiagnostic,
            ..Default::default()
        });

    assert!(apply_live_handoff_pr_link(
        &adapter,
        &issue.identifier,
        &mut result
    ));

    assert!(result.success);
    assert_eq!(
        result
            .live_handoff
            .as_ref()
            .and_then(|handoff| handoff.project_pr_link_verified),
        Some(true)
    );
}

#[test]
fn handoff_verification_skips_when_not_configured() {
    let config = test_config();
    let temp = tempfile::tempdir().unwrap();

    let verification = run_handoff_verification(temp.path(), &config);

    assert!(verification.success);
    assert_eq!(verification.summary, "skipped:not_configured");
}

#[test]
fn handoff_verification_runs_configured_commands() {
    let mut config = test_config();
    config.verification.commands = vec!["printf verified > verification.txt".into()];
    config.verification.timeout_ms = 5_000;
    let temp = tempfile::tempdir().unwrap();

    let verification = run_handoff_verification(temp.path(), &config);

    assert!(verification.success);
    assert_eq!(
        verification.summary,
        "passed:1 command(s) runtime_profile=not_configured"
    );
    assert_eq!(
        std::fs::read_to_string(temp.path().join("verification.txt")).unwrap(),
        "verified"
    );
}

#[test]
fn live_run_loop_handoff_link_failure_blocks_agent_review() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let mut result = successful_live_handoff_result(&handoff);
    let adapter = RecordingAdapter {
        fail_link_pr: true,
        ..Default::default()
    };

    assert!(!apply_live_handoff_pr_link(
        &adapter,
        &issue.identifier,
        &mut result
    ));

    assert!(!result.success);
    assert!(result.message.contains("handoff PR link repair failed"));
    assert_eq!(
        result
            .live_handoff
            .as_ref()
            .and_then(|handoff| handoff.project_pr_link_verified),
        Some(false)
    );
}

#[test]
fn live_run_loop_handoff_requires_verified_project_pr_linkage() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let mut result = successful_live_handoff_result(&handoff);
    let adapter = RecordingAdapter {
        confirm_link_pr: false,
        ..Default::default()
    };

    assert!(!apply_live_handoff_pr_link(
        &adapter,
        &issue.identifier,
        &mut result
    ));

    assert!(!result.success);
    assert!(result.message.contains("linked PR was not visible"));
    assert_eq!(
        result
            .live_handoff
            .as_ref()
            .and_then(|handoff| handoff.project_pr_link_verified),
        Some(false)
    );
}

fn successful_live_handoff_result(handoff: &IssueHandoffPlan) -> IssueExecutionResult {
    IssueExecutionResult {
        workspace_path: handoff.workspace_path.clone(),
        backend: "dry-run".into(),
        profile_id: None,
        instance_name: None,
        success: true,
        pending_session: false,
        session_id: Some("session-33".into()),
        run_id: None,
        backend_log_path: None,
        backend_attach_command: None,
        message: "ok".into(),
        usage_limit_pause: None,
        prompt_artifact_path: None,
        actor_role: "implementation_agent".into(),
        actor_label: "Shea Symphony Agent".into(),
        git_author: Some("Shea Symphony Agent <shea@example.invalid>".into()),
        git_identity: GitIdentityApplyResult {
            status: shea_symphony::workspace::GitIdentityApplyStatus::Applied,
            author: Some("Shea Symphony Agent <shea@example.invalid>".into()),
            applied_keys: vec!["user.name".into(), "user.email".into()],
        },
        live_handoff: Some(RunLoopLiveHandoff {
            worktree: LiveWorktreeResult {
                workspace_path: handoff.workspace_path.clone(),
                branch_name: handoff.branch_name.clone(),
                created: true,
            },
            publication: PullRequestPublication {
                branch_pushed: true,
                pr_url: "https://github.com/Alive24/shea-symphony/pull/45".into(),
                pr_created: true,
            },
            verification: "skipped:not_configured".into(),
            project_pr_link_verified: Some(true),
            pull_request_ready: Some(PullRequestReadyStatus {
                pr_url: "https://github.com/Alive24/shea-symphony/pull/45".into(),
                was_draft: false,
                marked_ready: false,
            }),
        }),
        handoff_verification: Some("skipped:not_configured".into()),
    }
}

fn failed_codex_result(message: &str) -> IssueExecutionResult {
    IssueExecutionResult {
        workspace_path: PathBuf::from("/tmp/shea/issue-439"),
        backend: "codex".into(),
        profile_id: None,
        instance_name: None,
        success: false,
        pending_session: false,
        session_id: Some("session-439".into()),
        run_id: None,
        backend_log_path: None,
        backend_attach_command: None,
        message: message.into(),
        usage_limit_pause: None,
        prompt_artifact_path: None,
        actor_role: "implementation_agent".into(),
        actor_label: "Shea Symphony Agent".into(),
        git_author: Some("Shea Symphony Agent <shea@example.invalid>".into()),
        git_identity: GitIdentityApplyResult {
            status: shea_symphony::workspace::GitIdentityApplyStatus::Applied,
            author: Some("Shea Symphony Agent <shea@example.invalid>".into()),
            applied_keys: vec!["user.name".into(), "user.email".into()],
        },
        live_handoff: None,
        handoff_verification: None,
    }
}

#[test]
fn failed_backend_live_handoff_salvage_accepts_app_server_stall() {
    let result = failed_codex_result("Codex app-server stalled waiting for turn event");

    assert!(failed_backend_can_use_live_handoff(&result));
}

#[test]
fn failed_backend_live_handoff_salvage_accepts_reconciled_session_stall_artifact() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("439.events.json");
    std::fs::write(
        &log_path,
        r#"{"event":"Failed { backend: \"codex\", error: \"Codex app-server stalled waiting for turn event\" }"}"#,
    )
    .unwrap();
    let mut result = failed_codex_result("main session failed: registry status failed");
    result.backend_log_path = Some(log_path);

    assert!(failed_backend_can_use_live_handoff(&result));
}

#[test]
fn failed_backend_live_handoff_salvage_rejects_usage_limit_pause() {
    let mut result = failed_codex_result("Codex app-server stalled waiting for turn event");
    result.usage_limit_pause = Some(UsageLimitPause {
        classifier: "usage_limit".into(),
        evidence: "usage limit reached".into(),
    });

    assert!(!failed_backend_can_use_live_handoff(&result));
}

#[test]
fn failed_backend_live_handoff_salvage_rejects_non_codex_backend() {
    let mut result = failed_codex_result("Codex app-server stalled waiting for turn event");
    result.backend = "tmux".into();

    assert!(!failed_backend_can_use_live_handoff(&result));
}

#[test]
fn failed_backend_live_handoff_salvage_rejects_generic_failure() {
    let result = failed_codex_result("verification failed after running npm test");

    assert!(!failed_backend_can_use_live_handoff(&result));
}

#[test]
fn failed_backend_live_handoff_salvage_rejects_reconciled_session_without_stall_artifact() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("439.events.json");
    std::fs::write(&log_path, r#"{"event":"Failed"}"#).unwrap();
    let mut result = failed_codex_result("main session failed: registry status failed");
    result.backend_log_path = Some(log_path);

    assert!(!failed_backend_can_use_live_handoff(&result));
}

#[test]
fn handoff_verification_failure_blocks_success() {
    let mut config = test_config();
    config.verification.commands = vec!["echo nope >&2; exit 7".into()];
    config.verification.timeout_ms = 5_000;
    let temp = tempfile::tempdir().unwrap();

    let verification = run_handoff_verification(temp.path(), &config);

    assert!(!verification.success);
    assert!(verification.summary.contains("failed command=`echo nope"));
    assert!(verification.summary.contains("status 7"));
}

#[test]
fn usage_limit_pause_workpad_preserves_tracker_state_boundary() {
    let issue = tracker_issue("In Progress");
    let result = IssueExecutionResult {
        workspace_path: PathBuf::from("/tmp/shea/issue-63"),
        backend: "codex".into(),
        profile_id: None,
        instance_name: None,
        success: false,
        pending_session: false,
        session_id: Some("session-63".into()),
        run_id: None,
        backend_log_path: None,
        backend_attach_command: None,
        message: "Codex subprocess exited with status 1".into(),
        usage_limit_pause: Some(UsageLimitPause {
            classifier: "usage_limit".into(),
            evidence: "usage limit reached".into(),
        }),
        prompt_artifact_path: None,
        actor_role: "implementation_agent".into(),
        actor_label: "Shea Symphony Agent".into(),
        git_author: Some("Shea Symphony Agent <shea@example.invalid>".into()),
        git_identity: GitIdentityApplyResult {
            status: shea_symphony::workspace::GitIdentityApplyStatus::NotGitRepository,
            author: None,
            applied_keys: Vec::new(),
        },
        live_handoff: None,
        handoff_verification: None,
    };
    let pause = result.usage_limit_pause.as_ref().unwrap();
    let workpad = run_loop_usage_limit_pause_workpad(None, &issue, &result, pause, 20_000);

    assert!(workpad.contains("### Recovery / Rework"));
    assert!(workpad.contains("Classifier: `usage_limit`"));
    assert!(workpad.contains("Tracker state was not advanced to `Agent Review`"));
    assert!(workpad.contains("Retry backoff: `20000ms`"));
}

#[test]
fn run_loop_agent_review_handoff_blocks_missing_pr_url() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let result = IssueExecutionResult {
        workspace_path: handoff.workspace_path.clone(),
        backend: "dry-run".into(),
        profile_id: None,
        instance_name: None,
        success: true,
        pending_session: false,
        session_id: Some("session-57".into()),
        run_id: None,
        backend_log_path: None,
        backend_attach_command: None,
        message: "ok".into(),
        usage_limit_pause: None,
        prompt_artifact_path: None,
        actor_role: "implementation_agent".into(),
        actor_label: "Shea Symphony Agent".into(),
        git_author: Some("Shea Symphony Agent <shea@example.invalid>".into()),
        git_identity: GitIdentityApplyResult {
            status: shea_symphony::workspace::GitIdentityApplyStatus::Applied,
            author: Some("Shea Symphony Agent <shea@example.invalid>".into()),
            applied_keys: vec!["user.name".into(), "user.email".into()],
        },
        live_handoff: None,
        handoff_verification: None,
    };

    let workpad = run_loop_handoff_workpad(None, &issue, &result, &handoff, None);
    let evidence =
        run_loop_agent_review_handoff_evidence(&issue, &result, &handoff, Some(&workpad));
    let report = evaluate_agent_review_handoff(&evidence);

    assert!(!report.is_ready());
    assert_eq!(report.target_state.as_deref(), Some("need_human_input"));
    assert!(evidence
        .no_pr_blocker
        .unwrap()
        .contains("No pull request URL"));
}

#[test]
fn run_loop_agent_review_handoff_passes_with_pr_url() {
    let config = test_config();
    let mut issue = tracker_issue("In Progress");
    issue
        .linked_pull_requests
        .push(shea_symphony::model::LinkedPullRequest {
            id: Some("PR_57".into()),
            number: Some(57),
            url: Some("https://github.com/Alive24/shea-symphony/pull/57".into()),
            state: Some("OPEN".into()),
            is_draft: Some(false),
            ..Default::default()
        });
    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let result = IssueExecutionResult {
        workspace_path: handoff.workspace_path.clone(),
        backend: "dry-run".into(),
        profile_id: None,
        instance_name: None,
        success: true,
        pending_session: false,
        session_id: Some("session-57".into()),
        run_id: None,
        backend_log_path: None,
        backend_attach_command: None,
        message: "ok".into(),
        usage_limit_pause: None,
        prompt_artifact_path: None,
        actor_role: "implementation_agent".into(),
        actor_label: "Shea Symphony Agent".into(),
        git_author: Some("Shea Symphony Agent <shea@example.invalid>".into()),
        git_identity: GitIdentityApplyResult {
            status: shea_symphony::workspace::GitIdentityApplyStatus::Applied,
            author: Some("Shea Symphony Agent <shea@example.invalid>".into()),
            applied_keys: vec!["user.name".into(), "user.email".into()],
        },
        live_handoff: None,
        handoff_verification: None,
    };

    let workpad = run_loop_handoff_workpad(None, &issue, &result, &handoff, None);
    let evidence =
        run_loop_agent_review_handoff_evidence(&issue, &result, &handoff, Some(&workpad));
    let report = evaluate_agent_review_handoff(&evidence);

    assert!(report.is_ready());
    assert_eq!(report.target_state.as_deref(), Some("agent_review"));
    assert_eq!(
        evidence.pull_request_url.as_deref(),
        Some("https://github.com/Alive24/shea-symphony/pull/57")
    );
}

#[test]
fn run_loop_agent_review_handoff_blocks_draft_pr_and_missing_workpad_evidence() {
    let config = test_config();
    let mut issue = tracker_issue("In Progress");
    issue
        .linked_pull_requests
        .push(shea_symphony::model::LinkedPullRequest {
            id: Some("PR_57".into()),
            number: Some(57),
            url: Some("https://github.com/Alive24/shea-symphony/pull/57".into()),
            state: Some("OPEN".into()),
            is_draft: Some(false),
            ..Default::default()
        });
    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let result = successful_live_handoff_result(&handoff);

    let missing_workpad_evidence =
        run_loop_agent_review_handoff_evidence(&issue, &result, &handoff, None);
    let missing_workpad_report = evaluate_agent_review_handoff(&missing_workpad_evidence);

    assert!(!missing_workpad_report.is_ready());
    assert!(missing_workpad_report
        .missing
        .contains(&"Main Workpad `### Plan`".into()));
    assert!(missing_workpad_report
        .missing
        .contains(&"Main Workpad `### Work Log`".into()));

    let mut draft_result = successful_live_handoff_result(&handoff);
    if let Some(live_handoff) = draft_result.live_handoff.as_mut() {
        live_handoff.pull_request_ready = Some(PullRequestReadyStatus {
            pr_url: "https://github.com/Alive24/shea-symphony/pull/45".into(),
            was_draft: true,
            marked_ready: false,
        });
    }
    let draft_workpad = run_loop_handoff_workpad(None, &issue, &draft_result, &handoff, None);
    let draft_evidence = run_loop_agent_review_handoff_evidence(
        &issue,
        &draft_result,
        &handoff,
        Some(&draft_workpad),
    );
    let draft_report = evaluate_agent_review_handoff(&draft_evidence);

    assert!(!draft_report.is_ready());
    assert!(draft_report
        .missing
        .contains(&"non-draft pull request".into()));
}
