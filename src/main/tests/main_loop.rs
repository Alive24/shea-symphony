use super::*;

fn active_runtime_state(identifier: &str) -> RuntimeState {
    let mut state = RuntimeState::active(
        RuntimeIssueState {
            id: "ISSUE_29".into(),
            identifier: identifier.into(),
        },
        "dry-run",
    );
    state.updated_at_ms = Some(1_000);
    state
}

fn runtime_reconcile_test_config(root: &Path) -> RuntimeConfig {
    let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            &format!(
                "---\ntracker:\n  kind: memory\nartifacts:\n  root: {:?}\n  namespace: Alive24/jade-symphony\nobservability:\n  logs_root: {:?}\n---\nPrompt",
                root.display().to_string(),
                root.join("logs").display().to_string()
            ),
        )
        .unwrap();
    RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap()
}

fn main_tmux_session_record(issue_identifier: &str, status: SessionStatus) -> AgentSessionRecord {
    AgentSessionRecord {
        issue_id: Some("ISSUE_338".into()),
        issue_identifier: Some(issue_identifier.into()),
        issue_title: Some("Reconcile completed main tmux sessions after handoff".into()),
        lane: "main".into(),
        run_id: Some("20260520T0403Z-issue338-main-c91b".into()),
        thread: None,
        session_source: Some("loop".into()),
        claim_value: None,
        actor_role: Some("codex".into()),
        actor_label: Some("Codex manual main issue-338".into()),
        git_author: None,
        profile_id: None,
        instance_name: None,
        worktree: PathBuf::from("/tmp/issue-338"),
        branch: Some("feature/issue-338".into()),
        backend: "tmux".into(),
        session_name: "jade-main-338-attempt-1-reconcile".into(),
        pane_target: "jade-main-338-attempt-1-reconcile".into(),
        prompt_artifact_path: PathBuf::from("/tmp/prompt.md"),
        log_path: PathBuf::from("/tmp/session.log"),
        attach_command: "tmux attach-session -t jade-main-338-attempt-1-reconcile".into(),
        attempt: 1,
        status,
        started_at_ms: 1_000,
        updated_at_ms: 1_000,
    }
}

fn init_clean_git_workspace(path: &Path) {
    let output = ProcessCommand::new("git")
        .arg("init")
        .arg(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn main_app_server_smoke_gate_is_ready_for_codex_app_server() {
    let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nmain_lane:\n  backend: codex\ncodex:\n  command: /opt/homebrew/bin/codex app-server\n  approval_policy: never\n---\nPrompt",
        )
        .unwrap();
    let config = RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

    let gate = main_app_server_smoke_gate(&config);

    assert_eq!(gate.backend, "codex");
    assert_eq!(gate.backend_source, "codex-app-server");
    assert_eq!(gate.command, "/opt/homebrew/bin/codex app-server");
    assert_eq!(gate.approval_policy, "never");
    assert!(gate.app_server_live_smoke_ready);
}

#[test]
fn main_app_server_smoke_gate_rejects_non_app_server_codex() {
    let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nmain_lane:\n  backend: codex\ncodex:\n  command: codex exec\n---\nPrompt",
        )
        .unwrap();
    let config = RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

    let gate = main_app_server_smoke_gate(&config);

    assert_eq!(gate.backend, "codex");
    assert_eq!(gate.backend_source, "codex-subprocess");
    assert!(!gate.app_server_live_smoke_ready);
    assert!(gate
        .app_server_live_smoke_reason
        .contains("does not select the app-server"));
}

#[test]
fn pool_worker_selection_respects_lane_priority_and_claim_owner() {
    let config = test_config();
    let worker = "Jade Symphony Main";
    let mut first = tracker_issue_with_ref("#1", "First", "Todo");
    first.priority = Some(20);
    let mut second = tracker_issue_with_ref("#2", "Second", "Rework");
    second.priority = Some(10);
    let mut owned_by_other = tracker_issue_with_ref("#3", "Other owned", "Todo");
    owned_by_other.project_fields.insert(
        "Main Agent".into(),
        serde_json::Value::String("Another Main".into()),
    );
    let mut owned_by_self = tracker_issue_with_ref("#4", "Self owned", "In Progress");
    owned_by_self.priority = Some(5);
    owned_by_self.project_fields.insert(
        "Main Agent".into(),
        serde_json::Value::String(worker.into()),
    );
    let merging = tracker_issue_with_ref("#5", "Merging", "Merging");

    let selected = select_pool_worker_issues(
        &[first, second, owned_by_other, owned_by_self, merging],
        WorkerLane::Main,
        worker,
        2,
        &config,
    );

    assert_eq!(
        selected
            .iter()
            .map(|issue| issue.identifier.as_str())
            .collect::<Vec<_>>(),
        vec!["#4", "#2"]
    );
}

#[test]
fn pool_worker_selection_returns_empty_when_no_slots_remain() {
    let config = test_config();
    let issue = tracker_issue_with_ref("#1", "Ready", "Todo");

    let selected = select_pool_worker_issues(&[issue], WorkerLane::Main, "worker", 0, &config);

    assert!(selected.is_empty());
}

#[test]
fn main_run_loop_selection_prioritizes_recovery_and_fills_remaining_slots() {
    let config = test_config();
    let worker = "Jade Symphony Main";
    let recovery_issue = tracker_issue_with_ref("#362", "Recover me", "In Progress");
    let mut next_todo = tracker_issue_with_ref("#363", "Start next", "Todo");
    next_todo.priority = Some(1);
    let recovery = RuntimeRecoveryCandidate {
        state: active_runtime_state("#362"),
        issue: recovery_issue,
        reason: "retry_due attempt=2 error=HTTP 429".into(),
    };

    let selected = select_main_run_loop_issues(&[recovery], &[next_todo], 2, worker, &config);

    assert_eq!(
        selected
            .iter()
            .map(|issue| issue.identifier.as_str())
            .collect::<Vec<_>>(),
        vec!["#362", "#363"]
    );
}

#[cfg(unix)]
#[test]
fn main_run_loop_write_dispatch_starts_selected_candidates_concurrently() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let codex = bin_dir.join("codex");
    let start_log = temp.path().join("codex-starts.log");
    std::fs::write(
            &codex,
            format!(
                r#"#!/bin/sh
set -eu
start_log={}
count=0
while IFS= read -r line; do
  count=$((count + 1))
  case "$count" in
    1)
      printf '%s\n' '{{"id":1,"result":{{}}}}'
      ;;
    2)
      ;;
    3)
      printf '{{"id":2,"result":{{"thread":{{"id":"thread-%s"}}}}}}\n' "$$"
      ;;
    4)
      printf '{{"id":3,"result":{{"turn":{{"id":"turn-%s"}}}}}}\n' "$$"
      printf '%s\n' "$$" >> "$start_log"
      remaining=40
      while [ "$remaining" -gt 0 ]; do
        starts="$(wc -l < "$start_log" 2>/dev/null || echo 0)"
        [ "$starts" -ge 2 ] && break
        remaining=$((remaining - 1))
        sleep 0.05
      done
      starts="$(wc -l < "$start_log" 2>/dev/null || echo 0)"
      if [ "$starts" -lt 2 ]; then
        printf '%s\n' '{{"method":"turn/failed","params":{{"error":{{"message":"second worker did not start before timeout"}}}}}}'
        exit 0
      fi
      printf '%s\n' '{{"method":"thread/tokenUsage/updated","params":{{"inputTokens":1,"outputTokens":1,"totalTokens":2}}}}'
      printf '%s\n' '{{"method":"turn/completed","params":{{"turn":{{"status":"completed"}}}}}}'
      exit 0
      ;;
  esac
done
"#,
                shell_quote_display(&start_log.display().to_string())
            ),
        )
        .unwrap();
    let mut permissions = std::fs::metadata(&codex).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&codex, permissions).unwrap();

    let workflow_path = temp.path().join("WORKFLOW.md");
    let workflow = WorkflowDefinition::parse(
            &workflow_path,
            &format!(
                "---\ntracker:\n  kind: memory\nworkspace:\n  root: {}\nartifacts:\n  root: {}\nobservability:\n  logs_root: {}\nmain_lane:\n  backend: codex\ncodex:\n  command: {} app-server\n  read_timeout_ms: 1000\n  turn_timeout_ms: 5000\n---\nPrompt for {{{{ issue.identifier }}}}",
                temp.path().join("worktrees").display(),
                temp.path().join("artifacts").display(),
                temp.path().join("logs").display(),
                shell_quote_display(&codex.display().to_string())
            ),
        )
        .unwrap();
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path).unwrap();
    let mut first = tracker_issue_with_ref("#362", "Recover first", "In Progress");
    first.id = "ISSUE_362".into();
    first.description = Some(forge_contract());
    let mut second = tracker_issue_with_ref("#363", "Start second", "In Progress");
    second.id = "ISSUE_363".into();
    second.description = Some(forge_contract());
    let options = RunLoopOptions {
        workflow_path,
        max_iterations: Some(1),
        once: false,
        write: true,
        recover: true,
        max_concurrent: Some(2),
        display: DisplayMode::Plain,
    };

    run_loop_dispatch_write_candidates(
        &workflow,
        &config,
        vec![first, second],
        &options,
        true,
        1,
        2,
    )
    .unwrap();

    let starts = std::fs::read_to_string(start_log).unwrap();
    assert_eq!(starts.lines().count(), 2);
}

#[test]
fn pool_claim_eligibility_reports_existing_owner() {
    let config = test_config();
    let mut issue = tracker_issue("Todo");
    issue.project_fields.insert(
        "Main Agent".into(),
        serde_json::Value::String("someone else".into()),
    );

    assert_eq!(
        pool_claim_eligibility(&issue, WorkerLane::Main, "this worker", &config),
        PoolClaimEligibility::ClaimedByOther {
            owner: "someone else".into()
        }
    );
}

#[test]
fn run_loop_claim_action_uses_tracker_claim_decision() {
    let config = test_config();

    assert_eq!(
        run_loop_claim_action(&tracker_issue("Todo"), &config),
        RunLoopClaimAction::Claim
    );
    assert_eq!(
        run_loop_claim_action(&tracker_issue("Rework"), &config),
        RunLoopClaimAction::Claim
    );
    assert_eq!(
        run_loop_claim_action(&tracker_issue("In Progress"), &config),
        RunLoopClaimAction::Resume
    );
    assert_eq!(
        run_loop_claim_action(&tracker_issue("Agent Review"), &config),
        RunLoopClaimAction::StopAndReplan {
            current_state: "Agent Review".into()
        }
    );
}

#[test]
fn live_gate_blocks_missing_assignee_without_override() {
    let config = live_github_config(false);
    let issue = tracker_issue("Todo");

    assert_eq!(
        live_missing_assignee_gate_blocker(&config, &issue).as_deref(),
        Some("live GitHub issue assignee")
    );
}

#[test]
fn fixture_mode_does_not_require_live_assignee() {
    let config = fixture_github_config();
    let issue = tracker_issue("Todo");

    assert_eq!(live_missing_assignee_gate_blocker(&config, &issue), None);
    assert_eq!(
        run_loop_assignee_ownership_decision(&issue, &config, None, None),
        AssigneeOwnershipDecision::Allowed
    );
}

#[test]
fn assignee_ownership_allows_matching_active_login() {
    let config = live_github_config(false);
    let mut issue = tracker_issue("Todo");
    issue.assignees = vec!["CodexUser".into()];

    assert_eq!(
        run_loop_assignee_ownership_decision(&issue, &config, Some("codexuser"), None),
        AssigneeOwnershipDecision::Allowed
    );
}

#[test]
fn assignee_ownership_blocks_mismatched_active_login() {
    let config = live_github_config(false);
    let mut issue = tracker_issue("Todo");
    issue.assignees = vec!["owner-a".into()];

    let decision = run_loop_assignee_ownership_decision(&issue, &config, Some("owner-b"), None);

    assert!(matches!(decision, AssigneeOwnershipDecision::Block { .. }));
}

#[test]
fn assignee_ownership_allows_matching_profile_login() {
    let config = live_github_config(false);
    let mut issue = tracker_issue("Todo");
    issue.assignees = vec!["profile-owner".into()];

    assert_eq!(
        run_loop_assignee_ownership_decision(
            &issue,
            &config,
            Some("different-gh-user"),
            Some("profile-owner"),
        ),
        AssigneeOwnershipDecision::Allowed
    );
}

#[test]
fn assignee_ownership_blocks_missing_active_identity() {
    let config = live_github_config(false);
    let mut issue = tracker_issue("Todo");
    issue.assignees = vec!["owner-a".into()];

    let decision = run_loop_assignee_ownership_decision(&issue, &config, None, None);

    assert_eq!(
        decision,
        AssigneeOwnershipDecision::Block {
            reason: "active GitHub identity unavailable for assignee ownership check".into(),
        }
    );
}

#[test]
fn run_loop_runtime_ownership_workpad_records_matching_marker() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let ownership = run_loop_runtime_ownership(&issue, &config, &handoff).unwrap();
    let claim = test_claim(&issue);

    let workpad = run_loop_ownership_workpad(&issue, &ownership, "Resumed", &claim);

    assert!(workpad.contains("jade-symphony-runtime-ownership"));
    assert_eq!(
        runtime_ownership_decision(Some(&workpad), &ownership),
        RuntimeOwnershipDecision::Matches
    );
}

#[test]
fn run_loop_runtime_ownership_detects_different_active_branch() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let expected = run_loop_runtime_ownership(&issue, &config, &handoff).unwrap();
    let mut existing = expected.clone();
    existing.branch_name = "feature/issue-100-other-work".into();
    let workpad = render_runtime_ownership_marker(&existing);

    assert!(matches!(
        runtime_ownership_decision(Some(&workpad), &expected),
        RuntimeOwnershipDecision::Mismatched { .. }
    ));
}

#[test]
fn resume_preflight_continues_active_in_progress_state() {
    let config = test_config();
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let state = active_runtime_state("#29");

    let action = run_loop_resume_preflight(&tracker, &config, Some(&state), 2_000).unwrap();

    assert_eq!(action, ResumePreflightAction::Continue);
}

#[test]
fn resume_preflight_archives_non_active_state_with_absent_worktree() {
    let config = test_config();
    let tracker = MemoryTracker::new(vec![tracker_issue("Need to Clarify")]);
    let state = active_runtime_state("#29");

    let action = run_loop_resume_preflight(&tracker, &config, Some(&state), 2_000).unwrap();

    assert_eq!(
        action,
        ResumePreflightAction::ArchiveStale {
            issue_identifier: "#29".into(),
            tracker_state: "Need to Clarify".into(),
            archive_reason: "tracker_state_non_active".into(),
        }
    );
}

#[test]
fn resume_preflight_archives_terminal_state_with_clean_worktree() {
    let config = test_config();
    let tracker = MemoryTracker::new(vec![tracker_issue("Done")]);
    let temp = tempfile::tempdir().unwrap();
    init_clean_git_workspace(temp.path());
    let mut state = active_runtime_state("#29");
    state.workspace_path = Some(temp.path().to_path_buf());

    let action = run_loop_resume_preflight(&tracker, &config, Some(&state), 2_000).unwrap();

    assert_eq!(
        action,
        ResumePreflightAction::ArchiveStale {
            issue_identifier: "#29".into(),
            tracker_state: "Done".into(),
            archive_reason: "tracker_state_terminal".into(),
        }
    );
}

#[test]
fn resume_preflight_blocks_non_active_state_with_dirty_worktree() {
    let config = test_config();
    let tracker = MemoryTracker::new(vec![tracker_issue("Need to Clarify")]);
    let temp = tempfile::tempdir().unwrap();
    init_clean_git_workspace(temp.path());
    std::fs::write(temp.path().join("scratch.txt"), "dirty work").unwrap();
    let mut state = active_runtime_state("#29");
    state.workspace_path = Some(temp.path().to_path_buf());

    let action = run_loop_resume_preflight(&tracker, &config, Some(&state), 2_000).unwrap();

    assert!(
        matches!(action, ResumePreflightAction::Block { reason } if reason.contains("workspace is dirty"))
    );
}

#[test]
fn resume_preflight_archive_allows_unrelated_todo_selection() {
    let config = main_loop_test_config();
    let stale = tracker_issue_with_ref("#29", "Needs clarification", "Need to Clarify");
    let mut todo = tracker_issue_with_ref("#30", "Ready next work", "Todo");
    todo.description = Some(forge_contract());
    let tracker = MemoryTracker::new(vec![stale, todo.clone()]);
    let state = active_runtime_state("#29");

    let action = run_loop_resume_preflight(&tracker, &config, Some(&state), 2_000).unwrap();
    let plan = Orchestrator::new(config).plan_dispatch(tracker.list_dispatchable_issues().unwrap());

    assert!(matches!(action, ResumePreflightAction::ArchiveStale { .. }));
    assert_eq!(
        plan.selected.first().map(|issue| issue.identifier.as_str()),
        Some("#30")
    );
}

#[test]
fn resume_preflight_many_counts_active_main_worker_slots() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    let tracker = MemoryTracker::new(vec![
        tracker_issue_with_ref("#29", "Runtime one", "In Progress"),
        tracker_issue_with_ref("#30", "Runtime two", "In Progress"),
    ]);
    let states = vec![active_runtime_state("#29"), active_runtime_state("#30")];

    let summary = run_loop_resume_preflight_many(&tracker, &config, &states, 2_000, false).unwrap();

    assert_eq!(summary.active_main_workers, 2);
    assert_eq!(summary.retained_states.len(), 2);
    assert_eq!(summary.blocked, None);
}

#[test]
fn resume_preflight_many_archives_only_stale_slot() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    let tracker = MemoryTracker::new(vec![
        tracker_issue_with_ref("#29", "Handed off", "Agent Review"),
        tracker_issue_with_ref("#30", "Still active", "In Progress"),
    ]);
    let states = vec![active_runtime_state("#29"), active_runtime_state("#30")];

    let summary = run_loop_resume_preflight_many(&tracker, &config, &states, 2_000, false).unwrap();

    assert_eq!(summary.active_main_workers, 1);
    assert_eq!(summary.retained_states.len(), 1);
    assert_eq!(
        summary.retained_states[0]
            .active_issue
            .as_ref()
            .map(|issue| issue.identifier.as_str()),
        Some("#30")
    );
}

#[test]
fn resume_preflight_many_marks_stalled_state_recoverable_when_requested() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut state = active_runtime_state("#29");
    state.updated_at_ms = Some(1_000);

    let summary = run_loop_resume_preflight_many(
        &tracker,
        &config,
        &[state],
        config.codex.stall_timeout_ms + 2_000,
        true,
    )
    .unwrap();

    assert_eq!(summary.active_main_workers, 0);
    assert_eq!(summary.blocked, None);
    assert_eq!(summary.retained_states.len(), 1);
    assert_eq!(summary.recoverable_states.len(), 1);
}

#[test]
fn resume_preflight_many_prefers_completed_app_server_session_over_stale_runtime_clock() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut state = active_runtime_state("#29");
    state.backend = "codex".into();
    state.last_event = Some("Resumed".into());
    state.backend_session_id = None;
    state.updated_at_ms = Some(1_000);

    let mut record = main_tmux_session_record("#29", SessionStatus::Completed);
    record.backend = "codex".into();
    record.session_source = Some("codex-app-server".into());
    record.session_name = "thread-29-turn-1".into();
    record.pane_target = String::new();
    record.log_path = temp.path().join("logs/app-server/29.events.json");
    record.updated_at_ms = 2_000;
    save_session_registry(
        &session_registry_path(&config),
        &jade_symphony::session_registry::SessionRegistry {
            sessions: vec![record],
        },
    )
    .unwrap();

    let summary = run_loop_resume_preflight_many(
        &tracker,
        &config,
        &[state],
        config.codex.stall_timeout_ms + 2_000,
        true,
    )
    .unwrap();

    assert_eq!(summary.active_main_workers, 0);
    assert_eq!(summary.blocked, None);
    assert_eq!(summary.recoverable_states.len(), 1);
    assert!(summary.recoverable_states[0]
        .reason
        .contains("status=completed"));
    assert_eq!(
        summary.recoverable_states[0].state.last_event.as_deref(),
        Some("SessionTerminal")
    );
    assert_eq!(
        summary.recoverable_states[0]
            .state
            .backend_session_id
            .as_deref(),
        Some("thread-29-turn-1")
    );
}

#[test]
fn resumed_pending_session_state_preserves_backend_session_for_reconciliation() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let claim = test_claim(&issue);
    let mut existing = active_runtime_state(&issue.identifier);
    existing.backend = "codex".into();
    existing.last_event = Some("SessionTerminal".into());
    existing.backend_session_id = Some("thread-29-turn-1".into());
    existing.backend_log_path = Some(PathBuf::from("/tmp/29.events.json"));
    existing.workspace_path = Some(PathBuf::from("/tmp/issue-29"));

    let state =
        run_loop_runtime_state_for_issue(Some(&existing), &issue, &config, "Resumed", &claim);

    assert_eq!(state.last_event.as_deref(), Some("SessionTerminal"));
    assert_eq!(
        state.backend_session_id.as_deref(),
        Some("thread-29-turn-1")
    );
    assert_eq!(state.attempt_count, existing.attempt_count);
}

#[test]
fn resume_preflight_many_marks_missing_session_registry_recoverable() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut state = active_runtime_state("#29");
    state.backend = "tmux".into();
    state.last_event = Some("SessionRunning".into());
    state.backend_session_id = Some("jade-main-missing".into());

    let summary = run_loop_resume_preflight_many(&tracker, &config, &[state], 2_000, true).unwrap();

    assert_eq!(summary.active_main_workers, 0);
    assert_eq!(summary.blocked, None);
    assert_eq!(summary.recoverable_states.len(), 1);
}

#[cfg(unix)]
#[test]
fn resume_preflight_many_counts_running_tmux_session_in_recover_mode() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    config.tmux.command = fake_tmux_capture_script(
        temp.path(),
        "Codex\n◦ Running cargo run -- autopilot plan\n› Improve documentation in @filename",
    );
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut state = active_runtime_state("#29");
    state.backend = "tmux".into();
    state.last_event = Some("SessionRunning".into());
    state.backend_session_id = Some("jade-main-29-attempt-1".into());
    state.updated_at_ms = Some(1_000);

    let mut record = main_tmux_session_record("#29", SessionStatus::Running);
    record.session_name = "jade-main-29-attempt-1".into();
    record.pane_target = "jade-main-29-attempt-1".into();
    record.log_path = temp.path().join("session.log");
    record.updated_at_ms = 1_000;
    save_session_registry(
        &session_registry_path(&config),
        &jade_symphony::session_registry::SessionRegistry {
            sessions: vec![record],
        },
    )
    .unwrap();

    let summary = run_loop_resume_preflight_many(
        &tracker,
        &config,
        &[state],
        config.codex.stall_timeout_ms + 2_000,
        true,
    )
    .unwrap();

    assert_eq!(summary.active_main_workers, 1);
    assert_eq!(summary.blocked, None);
    assert_eq!(summary.retained_states.len(), 1);
    assert_eq!(summary.recoverable_states.len(), 0);
}

#[cfg(unix)]
#[test]
fn resume_preflight_many_counts_registry_only_running_tmux_session_in_recover_mode() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    config.tmux.command = fake_tmux_capture_script(
        temp.path(),
        "Codex\n◦ Running cargo test\n› Improve documentation in @filename",
    );
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut record = main_tmux_session_record("#29", SessionStatus::Running);
    record.session_name = "jade-main-29-attempt-1".into();
    record.pane_target = "jade-main-29-attempt-1".into();
    record.log_path = temp.path().join("session.log");
    record.updated_at_ms = 1_000;
    save_session_registry(
        &session_registry_path(&config),
        &jade_symphony::session_registry::SessionRegistry {
            sessions: vec![record],
        },
    )
    .unwrap();

    let summary = run_loop_resume_preflight_many(
        &tracker,
        &config,
        &[],
        config.codex.stall_timeout_ms + 2_000,
        true,
    )
    .unwrap();

    assert_eq!(summary.active_main_workers, 1);
    assert_eq!(summary.blocked, None);
    assert_eq!(summary.retained_states.len(), 1);
    assert_eq!(summary.recoverable_states.len(), 0);
    assert_eq!(
        runtime_state_issue_identifier(&summary.retained_states[0]),
        Some("#29")
    );
}

#[test]
fn resume_preflight_many_recovers_registry_only_failed_app_server_session() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut record = main_tmux_session_record("#29", SessionStatus::Failed);
    record.backend = "codex".into();
    record.session_source = Some("codex-app-server".into());
    record.session_name = "thread-29-turn-1".into();
    record.pane_target = String::new();
    record.attach_command =
        "not a tmux session; inspect app-server artifacts for recovery evidence".into();
    record.log_path = temp.path().join("logs/app-server/29.events.json");
    save_session_registry(
        &session_registry_path(&config),
        &jade_symphony::session_registry::SessionRegistry {
            sessions: vec![record],
        },
    )
    .unwrap();

    let summary = run_loop_resume_preflight_many(&tracker, &config, &[], 2_000, true).unwrap();

    assert_eq!(summary.active_main_workers, 0);
    assert_eq!(summary.blocked, None);
    assert_eq!(summary.retained_states.len(), 1);
    assert_eq!(summary.recoverable_states.len(), 1);
    assert!(summary.recoverable_states[0]
        .reason
        .contains("status=failed"));
    assert_eq!(
        runtime_state_issue_identifier(&summary.retained_states[0]),
        Some("#29")
    );
}

#[cfg(unix)]
#[test]
fn resume_preflight_registry_active_session_does_not_require_live_issue_read() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    config.tmux.command = fake_tmux_capture_script(
        temp.path(),
        "Codex\n◦ Running cargo test\n› Improve documentation in @filename",
    );
    let tracker = RecordingAdapter {
        fail_get_issue: true,
        ..Default::default()
    };
    let mut record = main_tmux_session_record("#29", SessionStatus::Running);
    record.session_name = "jade-main-29-attempt-1".into();
    record.pane_target = "jade-main-29-attempt-1".into();
    record.log_path = temp.path().join("session.log");
    save_session_registry(
        &session_registry_path(&config),
        &jade_symphony::session_registry::SessionRegistry {
            sessions: vec![record],
        },
    )
    .unwrap();

    let summary = run_loop_resume_preflight_many(&tracker, &config, &[], 2_000, true).unwrap();

    assert_eq!(summary.active_main_workers, 1);
    assert_eq!(summary.recoverable_states.len(), 0);
    assert_eq!(summary.retained_states.len(), 1);
}

#[cfg(unix)]
#[test]
fn resume_preflight_registry_active_session_skips_non_in_progress_tracker_state() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    config.tmux.command = fake_tmux_capture_script(
        temp.path(),
        "Codex\n◦ Running cargo test\n› Improve documentation in @filename",
    );
    let tracker = MemoryTracker::new(vec![tracker_issue("Agent Review")]);
    let mut record = main_tmux_session_record("#29", SessionStatus::Running);
    record.session_name = "jade-main-29-attempt-1".into();
    record.pane_target = "jade-main-29-attempt-1".into();
    record.log_path = temp.path().join("session.log");
    save_session_registry(
        &session_registry_path(&config),
        &jade_symphony::session_registry::SessionRegistry {
            sessions: vec![record],
        },
    )
    .unwrap();

    let summary = run_loop_resume_preflight_many(&tracker, &config, &[], 2_000, true).unwrap();

    assert_eq!(summary.active_main_workers, 0);
    assert_eq!(summary.recoverable_states.len(), 0);
    assert_eq!(summary.retained_states.len(), 0);
}

#[cfg(unix)]
#[test]
fn resume_preflight_prefers_running_sibling_session_over_interrupted_runtime_session() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    config.tmux.command = fake_tmux_split_session_script(temp.path());
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut attempt_two = main_tmux_session_record("#29", SessionStatus::Running);
    attempt_two.session_name = "jade-main-29-attempt-2".into();
    attempt_two.pane_target = "jade-main-29-attempt-2".into();
    attempt_two.log_path = temp.path().join("attempt-2.log");
    attempt_two.attempt = 2;
    let mut attempt_three = main_tmux_session_record("#29", SessionStatus::Running);
    attempt_three.session_name = "jade-main-29-attempt-3".into();
    attempt_three.pane_target = "jade-main-29-attempt-3".into();
    attempt_three.log_path = temp.path().join("attempt-3.log");
    attempt_three.attempt = 3;
    save_session_registry(
        &session_registry_path(&config),
        &jade_symphony::session_registry::SessionRegistry {
            sessions: vec![attempt_two, attempt_three],
        },
    )
    .unwrap();
    let mut state = active_runtime_state("#29");
    state.backend = "tmux".into();
    state.last_event = Some("SessionRunning".into());
    state.backend_session_id = Some("jade-main-29-attempt-3".into());
    state.updated_at_ms = Some(1_000);

    let summary = run_loop_resume_preflight_many(
        &tracker,
        &config,
        &[state],
        config.codex.stall_timeout_ms + 2_000,
        true,
    )
    .unwrap();

    assert_eq!(summary.active_main_workers, 1);
    assert_eq!(summary.recoverable_states.len(), 0);
    assert_eq!(
        summary.retained_states[0].backend_session_id.as_deref(),
        Some("jade-main-29-attempt-2")
    );
}

#[cfg(unix)]
#[test]
fn resume_preflight_many_recovers_registry_only_unavailable_tmux_session() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    config.tmux.command = fake_tmux_unavailable_script(temp.path());
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut record = main_tmux_session_record("#29", SessionStatus::Running);
    record.session_name = "jade-main-29-attempt-1".into();
    record.pane_target = "jade-main-29-attempt-1".into();
    record.log_path = temp.path().join("session.log");
    save_session_registry(
        &session_registry_path(&config),
        &jade_symphony::session_registry::SessionRegistry {
            sessions: vec![record],
        },
    )
    .unwrap();

    let summary = run_loop_resume_preflight_many(&tracker, &config, &[], 2_000, true).unwrap();

    assert_eq!(summary.active_main_workers, 0);
    assert_eq!(summary.blocked, None);
    assert_eq!(summary.retained_states.len(), 1);
    assert_eq!(summary.recoverable_states.len(), 1);
    assert!(summary.recoverable_states[0]
        .reason
        .contains("registry_session_unavailable"));
}

#[cfg(unix)]
#[test]
fn resume_preflight_many_recovers_runtime_state_unavailable_tmux_session() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    config.tmux.command = fake_tmux_unavailable_script(temp.path());
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut record = main_tmux_session_record("#29", SessionStatus::Running);
    record.session_name = "jade-main-29-attempt-1".into();
    record.pane_target = "jade-main-29-attempt-1".into();
    record.log_path = temp.path().join("session.log");
    save_session_registry(
        &session_registry_path(&config),
        &jade_symphony::session_registry::SessionRegistry {
            sessions: vec![record],
        },
    )
    .unwrap();
    let mut state = active_runtime_state("#29");
    state.backend = "tmux".into();
    state.last_event = Some("SessionRunning".into());
    state.backend_session_id = Some("jade-main-29-attempt-1".into());

    let summary = run_loop_resume_preflight_many(&tracker, &config, &[state], 2_000, true).unwrap();

    assert_eq!(summary.active_main_workers, 0);
    assert_eq!(summary.blocked, None);
    assert_eq!(summary.retained_states.len(), 1);
    assert_eq!(summary.recoverable_states.len(), 1);
    assert!(summary.recoverable_states[0]
        .reason
        .contains("tmux_pane_unavailable"));
}

#[cfg(unix)]
fn fake_tmux_capture_script(root: &Path, output: &str) -> String {
    use std::os::unix::fs::PermissionsExt;

    let path = root.join("fake-tmux");
    std::fs::write(
            &path,
            format!(
                "#!/bin/sh\nif [ \"${{1:-}}\" = \"capture-pane\" ]; then\ncat <<'EOF'\n{output}\nEOF\nexit 0\nfi\nexit 0\n"
            ),
        )
        .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path.display().to_string()
}

#[cfg(unix)]
fn fake_tmux_split_session_script(root: &Path) -> String {
    use std::os::unix::fs::PermissionsExt;

    let path = root.join("fake-tmux-split-session");
    std::fs::write(
            &path,
            r#"#!/bin/sh
case "$*" in
  *attempt-2*) printf '%s\n' 'Codex' '◦ Running cargo test' '› Improve documentation in @filename'; exit 0 ;;
  *attempt-3*) printf '%s\n' 'Conversation interrupted - tell the model what to do differently.' '› Write tests for @filename'; exit 0 ;;
esac
exit 1
"#,
        )
        .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path.display().to_string()
}

#[cfg(unix)]
fn fake_tmux_unavailable_script(root: &Path) -> String {
    use std::os::unix::fs::PermissionsExt;

    let path = root.join("fake-tmux-unavailable");
    std::fs::write(
        &path,
        "#!/bin/sh\nif [ \"${1:-}\" = \"capture-pane\" ]; then\nexit 1\nfi\nexit 0\n",
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path.display().to_string()
}

#[test]
fn recovery_handoff_reuses_dirty_existing_issue_worktree() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.workspace.root = temp.path().join("worktrees");
    std::fs::create_dir_all(&config.workspace.root).unwrap();
    let issue = tracker_issue("In Progress");
    let mut handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let worktree = config.workspace.root.join("_29-main-agent");
    init_clean_git_workspace(&worktree);
    git_ok(
        &worktree,
        &["checkout", "-b", "feature/issue-29-runtime-state-main-loop"],
    );
    std::fs::write(worktree.join("scratch.txt"), "dirty recovery work").unwrap();
    let mut state = active_runtime_state("#29");
    state.last_event = Some("SessionRunning".into());
    state.workspace_path = Some(worktree.clone());

    let evidence = run_loop_apply_recovery_handoff(&config, &issue, &mut handoff, &state).unwrap();

    assert!(evidence
        .as_deref()
        .unwrap()
        .contains("source=runtime_state"));
    assert_eq!(handoff.workspace_path, worktree);
    assert_eq!(handoff.workspace_key, "_29-main-agent");
    assert_eq!(
        handoff.branch_name,
        "feature/issue-29-runtime-state-main-loop"
    );
}

#[test]
fn run_loop_runtime_state_uses_matching_slot_for_attempt_count() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let claim = test_claim(&issue);
    let unrelated = active_runtime_state("#28");
    let existing = run_loop_runtime_state_for_issue(None, &issue, &config, "Claimed", &claim);
    let states = vec![unrelated, existing];

    let state = run_loop_runtime_state_for_issue(
        runtime_state_for_issue(&states, &issue.identifier),
        &issue,
        &config,
        "Resumed",
        &claim,
    );

    assert_eq!(state.attempt_count, 2);
    assert_eq!(
        state
            .active_issue
            .as_ref()
            .map(|issue| issue.identifier.as_str()),
        Some("#29")
    );
}

#[test]
fn resume_preflight_defers_until_retry_is_due() {
    let config = test_config();
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut state = active_runtime_state("#29");
    record_runtime_retry(&mut state, 1_000, 5_000, "rate limited");

    let action = run_loop_resume_preflight(&tracker, &config, Some(&state), 2_000).unwrap();

    assert!(matches!(
        action,
        ResumePreflightAction::RetryLater {
            due_in_ms: 4_000,
            ..
        }
    ));
}

#[test]
fn resume_preflight_continues_after_retry_is_due_even_when_old() {
    let config = test_config();
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut state = active_runtime_state("#29");
    record_runtime_retry(&mut state, 1_000, 5_000, "backend not ready");

    let action = run_loop_resume_preflight(
        &tracker,
        &config,
        Some(&state),
        config.codex.stall_timeout_ms + 10_000,
    )
    .unwrap();

    assert_eq!(action, ResumePreflightAction::Continue);
}

#[test]
fn resume_preflight_many_marks_due_retry_recoverable_when_requested() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut state = active_runtime_state("#29");
    record_runtime_retry(&mut state, 1_000, 5_000, "HTTP 429 too many requests");

    let summary = run_loop_resume_preflight_many(&tracker, &config, &[state], 7_000, true).unwrap();

    assert_eq!(summary.active_main_workers, 0);
    assert_eq!(summary.recoverable_states.len(), 1);
    assert!(summary.recoverable_states[0]
        .reason
        .contains("retry_due attempt="));
}

#[test]
fn resume_preflight_detects_stalled_active_state() {
    let config = test_config();
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut state = active_runtime_state("#29");
    state.updated_at_ms = Some(1_000);

    let action = run_loop_resume_preflight(
        &tracker,
        &config,
        Some(&state),
        config.codex.stall_timeout_ms + 2_000,
    )
    .unwrap();

    assert!(matches!(action, ResumePreflightAction::Stalled { .. }));
}

#[test]
fn resume_preflight_archives_completed_tracker_state() {
    let config = test_config();
    let tracker = MemoryTracker::new(vec![tracker_issue("Agent Review")]);
    let state = active_runtime_state("#29");

    let action = run_loop_resume_preflight(&tracker, &config, Some(&state), 2_000).unwrap();

    assert_eq!(
        action,
        ResumePreflightAction::ArchiveStale {
            issue_identifier: "#29".into(),
            tracker_state: "Agent Review".into(),
            archive_reason: "tracker_state_handoff".into(),
        }
    );
}

#[test]
fn main_handoff_reconcile_completes_session_and_clears_matching_runtime_state() {
    let temp = tempfile::tempdir().unwrap();
    let config = runtime_reconcile_test_config(temp.path());
    let mut state = active_runtime_state("#338");
    state.backend = "tmux".into();
    state.backend_session_id = Some("jade-main-338-attempt-1-reconcile".into());
    state.lane = Some("main".into());
    upsert_runtime_state(&config, &state).unwrap();
    save_session_registry(
        &session_registry_path(&config),
        &jade_symphony::session_registry::SessionRegistry {
            sessions: vec![main_tmux_session_record("#338", SessionStatus::Running)],
        },
    )
    .unwrap();

    reconcile_main_handoff_runtime_state(&config, "#338", "agent_review").unwrap();

    let runtime_states = load_runtime_states(&config).unwrap();
    assert!(runtime_state_for_issue(&runtime_states, "#338").is_none());
    let registry = load_session_registry(&session_registry_path(&config)).unwrap();
    assert_eq!(registry.sessions[0].status, SessionStatus::Completed);
    assert!(registry.sessions[0].updated_at_ms > 1_000);
}

#[test]
fn main_handoff_reconcile_does_not_clear_non_main_runtime_state() {
    let temp = tempfile::tempdir().unwrap();
    let config = runtime_reconcile_test_config(temp.path());
    let mut state = active_runtime_state("#338");
    state.backend = "tmux".into();
    state.backend_session_id = Some("jade-review-338-attempt-1-review".into());
    state.lane = Some("review".into());
    upsert_runtime_state(&config, &state).unwrap();

    reconcile_main_handoff_runtime_state(&config, "#338", "agent_review").unwrap();

    let runtime_states = load_runtime_states(&config).unwrap();
    assert_eq!(
        runtime_state_for_issue(&runtime_states, "#338"),
        Some(&state)
    );
}

#[test]
fn run_loop_runtime_state_increments_same_issue_attempts() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let claim = test_claim(&issue);
    let existing = run_loop_runtime_state_for_issue(None, &issue, &config, "Claimed", &claim);

    let state =
        run_loop_runtime_state_for_issue(Some(&existing), &issue, &config, "Resumed", &claim);

    assert_eq!(state.attempt_count, 2);
    assert_eq!(
        state
            .active_issue
            .as_ref()
            .map(|issue| issue.identifier.as_str()),
        Some("#29")
    );
    assert_eq!(state.branch_name, issue.branch_name);
    assert_eq!(state.actor_role.as_deref(), Some("implementation_agent"));
    assert_eq!(state.actor_label.as_deref(), Some("Jade Symphony Agent"));
    assert_eq!(state.last_event.as_deref(), Some("Resumed"));
}

#[test]
fn run_loop_runtime_state_records_result_and_transition() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let claim = test_claim(&issue);
    let state = run_loop_runtime_state_for_issue(None, &issue, &config, "Claimed", &claim);
    let result = IssueExecutionResult {
        workspace_path: PathBuf::from("/tmp/jade/issue-29"),
        backend: "dry-run".into(),
        profile_id: Some("codex-alpha".into()),
        instance_name: Some("Codex Alpha".into()),
        success: true,
        pending_session: false,
        session_id: Some("session-29".into()),
        run_id: Some(claim.run.clone()),
        backend_log_path: None,
        backend_attach_command: None,
        message: "ok".into(),
        usage_limit_pause: None,
        prompt_artifact_path: None,
        actor_role: "implementation_agent".into(),
        actor_label: "Jade Symphony Agent".into(),
        git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
        git_identity: GitIdentityApplyResult {
            status: jade_symphony::workspace::GitIdentityApplyStatus::Applied,
            author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
            applied_keys: vec!["user.name".into(), "user.email".into()],
        },
        live_handoff: None,
        handoff_verification: None,
    };

    let state = run_loop_runtime_state_with_result(state, &result);
    assert_eq!(state.workspace_path, Some(result.workspace_path));
    assert_eq!(state.backend_session_id.as_deref(), Some("session-29"));
    assert_eq!(state.profile_id.as_deref(), Some("codex-alpha"));
    assert_eq!(state.actor_role.as_deref(), Some("implementation_agent"));
    assert_eq!(
        state.git_author.as_deref(),
        Some("Jade Symphony Agent <jade@example.invalid>")
    );
    assert_eq!(state.last_event.as_deref(), Some("Completed"));

    let state = run_loop_runtime_state_with_transition(
        state,
        Some("In Progress".into()),
        "agent_review",
        "main agent completed",
    );
    assert_eq!(
        state.last_transition,
        Some(RuntimeTransition {
            from: Some("In Progress".into()),
            to: "agent_review".into(),
            reason: "main agent completed".into(),
        })
    );
}

#[test]
fn run_loop_runtime_state_records_pending_tmux_session_metadata() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let claim = test_claim(&issue);
    let state = run_loop_runtime_state_for_issue(None, &issue, &config, "Claimed", &claim);
    let result = IssueExecutionResult {
        workspace_path: PathBuf::from("/tmp/jade/issue-220"),
        backend: "tmux".into(),
        profile_id: None,
        instance_name: None,
        success: false,
        pending_session: true,
        session_id: Some("jade-main-220".into()),
        run_id: Some(claim.run.clone()),
        backend_log_path: Some(PathBuf::from("/tmp/jade/logs/tmux/jade-main-220.log")),
        backend_attach_command: Some("tmux attach-session -t jade-main-220".into()),
        message: "tmux session running".into(),
        usage_limit_pause: None,
        prompt_artifact_path: None,
        actor_role: "implementation_agent".into(),
        actor_label: "Jade Symphony Agent".into(),
        git_author: None,
        git_identity: GitIdentityApplyResult {
            status: jade_symphony::workspace::GitIdentityApplyStatus::NotGitRepository,
            author: None,
            applied_keys: Vec::new(),
        },
        live_handoff: None,
        handoff_verification: None,
    };

    let state = run_loop_runtime_state_with_result(state, &result);
    let workpad = run_loop_handoff_workpad(
        &issue,
        &result,
        &run_loop_handoff_plan(&config, &issue).unwrap(),
        None,
    );

    assert_eq!(state.last_event.as_deref(), Some("SessionRunning"));
    assert_eq!(state.backend_session_id.as_deref(), Some("jade-main-220"));
    assert_eq!(
        state.backend_attach_command.as_deref(),
        Some("tmux attach-session -t jade-main-220")
    );
    assert!(workpad.contains("Session status: `running`"));
    assert!(workpad.contains("Attach command: `tmux attach-session -t jade-main-220`"));
    assert!(workpad.contains("Session log: `/tmp/jade/logs/tmux/jade-main-220.log`"));
}

#[test]
fn main_loop_reconciles_completed_pending_session_without_relaunching_backend() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = main_loop_test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    let issue = tracker_issue("In Progress");
    let claim = test_claim(&issue);
    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let mut state = run_loop_runtime_state_for_issue(None, &issue, &config, "Claimed", &claim);
    state.last_event = Some("SessionRunning".into());
    state.workspace_path = Some(handoff.workspace_path.clone());
    state.backend = "tmux".into();
    state.backend_session_id = Some("jade-main-29".into());
    state.backend_attach_command = Some("tmux attach-session -t jade-main-29".into());
    state.backend_log_path = Some(temp.path().join("jade-main-29.log"));

    save_session_record(
        &session_registry_path(&config),
        AgentSessionRecord {
            issue_id: Some(issue.id.clone()),
            issue_identifier: Some(issue.identifier.clone()),
            issue_title: Some(issue.title.clone()),
            lane: "main".into(),
            run_id: state.run_id.clone(),
            thread: None,
            session_source: None,
            claim_value: None,
            actor_role: state.actor_role.clone(),
            actor_label: state.actor_label.clone(),
            git_author: state.git_author.clone(),
            profile_id: state.profile_id.clone(),
            instance_name: state.instance_name.clone(),
            worktree: handoff.workspace_path.clone(),
            branch: Some(handoff.branch_name.clone()),
            backend: "codex".into(),
            session_name: "jade-main-29".into(),
            pane_target: String::new(),
            prompt_artifact_path: temp.path().join("prompt.md"),
            log_path: temp.path().join("jade-main-29.log"),
            attach_command: "tmux attach-session -t jade-main-29".into(),
            attempt: 1,
            status: SessionStatus::Completed,
            started_at_ms: 1,
            updated_at_ms: 2,
        },
    )
    .unwrap();

    let reconciliation = reconcile_pending_main_session(&config, &issue, &handoff, &state)
        .unwrap()
        .expect("expected completed session reconciliation");

    let MainSessionReconciliation::Terminal(result) = reconciliation else {
        panic!("expected terminal completed reconciliation");
    };
    assert!(result.success);
    assert!(!result.pending_session);
    assert_eq!(result.session_id.as_deref(), Some("jade-main-29"));
    assert!(result.message.contains("registry status completed"));
}

#[test]
fn main_loop_keeps_missing_pending_session_registry_active_instead_of_relaunching() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = main_loop_test_config();
    config.artifacts.root = temp.path().join("artifacts");
    let issue = tracker_issue("In Progress");
    let claim = test_claim(&issue);
    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let mut state = run_loop_runtime_state_for_issue(None, &issue, &config, "Claimed", &claim);
    state.last_event = Some("SessionRunning".into());
    state.backend_session_id = Some("jade-main-missing".into());

    let reconciliation = reconcile_pending_main_session(&config, &issue, &handoff, &state)
        .unwrap()
        .expect("expected active missing-registry reconciliation");

    let MainSessionReconciliation::Active {
        status,
        source,
        evidence,
    } = reconciliation
    else {
        panic!("expected active reconciliation");
    };
    assert_eq!(status, "unknown");
    assert_eq!(source, "runtime");
    assert!(evidence.contains("missing from session registry"));
}

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
        actor_label: "Jade Symphony Agent".into(),
        git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
        git_identity: GitIdentityApplyResult {
            status: jade_symphony::workspace::GitIdentityApplyStatus::Applied,
            author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
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
                pr_url: "https://github.com/Alive24/jade-symphony/pull/45".into(),
                pr_created: true,
            },
            verification: "skipped:not_configured".into(),
            project_pr_link_verified: Some(true),
            pull_request_ready: Some(PullRequestReadyStatus {
                pr_url: "https://github.com/Alive24/jade-symphony/pull/45".into(),
                was_draft: false,
                marked_ready: false,
            }),
        }),
        handoff_verification: Some("skipped:not_configured".into()),
    };

    let workpad = run_loop_handoff_workpad(&issue, &result, &handoff, None);

    assert!(workpad.contains("### Plan"));
    assert!(workpad.contains("### Work Log"));
    assert!(workpad.contains("- [x] Read the issue contract"));
    assert!(workpad.contains("### Planned Handoff"));
    assert!(workpad.contains("Actor role: `implementation_agent`"));
    assert!(workpad.contains("Git identity: `applied:Jade Symphony Agent <jade@example.invalid>`"));
    assert!(
        workpad.contains("Workspace key: `issue-29-wire-runtime-state-persistence-into-main-loop`")
    );
    assert!(workpad
        .contains("Branch: `feature/issue-29-wire-runtime-state-persistence-into-main-loop`"));
    assert!(workpad.contains("PR title: `#29: Wire runtime state persistence into main loop`"));
    assert!(workpad.contains("Handoff verification: `skipped:not_configured`"));
    assert!(workpad.contains("Live PR: `https://github.com/Alive24/jade-symphony/pull/45`"));
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
        vec!["link_pr:#29:https://github.com/Alive24/jade-symphony/pull/45"]
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
        .push(jade_symphony::model::LinkedPullRequest {
            number: Some(45),
            url: Some("https://github.com/Alive24/jade-symphony/pull/45".into()),
            state: Some("OPEN".into()),
            is_draft: Some(false),
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
    assert_eq!(verification.summary, "passed:1 command(s)");
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
    assert!(result.message.contains("not Project-visible"));
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
        actor_label: "Jade Symphony Agent".into(),
        git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
        git_identity: GitIdentityApplyResult {
            status: jade_symphony::workspace::GitIdentityApplyStatus::Applied,
            author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
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
                pr_url: "https://github.com/Alive24/jade-symphony/pull/45".into(),
                pr_created: true,
            },
            verification: "skipped:not_configured".into(),
            project_pr_link_verified: Some(true),
            pull_request_ready: Some(PullRequestReadyStatus {
                pr_url: "https://github.com/Alive24/jade-symphony/pull/45".into(),
                was_draft: false,
                marked_ready: false,
            }),
        }),
        handoff_verification: Some("skipped:not_configured".into()),
    }
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
        workspace_path: PathBuf::from("/tmp/jade/issue-63"),
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
        actor_label: "Jade Symphony Agent".into(),
        git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
        git_identity: GitIdentityApplyResult {
            status: jade_symphony::workspace::GitIdentityApplyStatus::NotGitRepository,
            author: None,
            applied_keys: Vec::new(),
        },
        live_handoff: None,
        handoff_verification: None,
    };
    let pause = result.usage_limit_pause.as_ref().unwrap();
    let workpad = run_loop_usage_limit_pause_workpad(&issue, &result, pause, 20_000);

    assert!(workpad.contains("### Usage-Limit Pause"));
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
        actor_label: "Jade Symphony Agent".into(),
        git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
        git_identity: GitIdentityApplyResult {
            status: jade_symphony::workspace::GitIdentityApplyStatus::Applied,
            author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
            applied_keys: vec!["user.name".into(), "user.email".into()],
        },
        live_handoff: None,
        handoff_verification: None,
    };

    let workpad = run_loop_handoff_workpad(&issue, &result, &handoff, None);
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
        .push(jade_symphony::model::LinkedPullRequest {
            id: Some("PR_57".into()),
            number: Some(57),
            url: Some("https://github.com/Alive24/jade-symphony/pull/57".into()),
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
        actor_label: "Jade Symphony Agent".into(),
        git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
        git_identity: GitIdentityApplyResult {
            status: jade_symphony::workspace::GitIdentityApplyStatus::Applied,
            author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
            applied_keys: vec!["user.name".into(), "user.email".into()],
        },
        live_handoff: None,
        handoff_verification: None,
    };

    let workpad = run_loop_handoff_workpad(&issue, &result, &handoff, None);
    let evidence =
        run_loop_agent_review_handoff_evidence(&issue, &result, &handoff, Some(&workpad));
    let report = evaluate_agent_review_handoff(&evidence);

    assert!(report.is_ready());
    assert_eq!(report.target_state.as_deref(), Some("agent_review"));
    assert_eq!(
        evidence.pull_request_url.as_deref(),
        Some("https://github.com/Alive24/jade-symphony/pull/57")
    );
}

#[test]
fn run_loop_agent_review_handoff_blocks_draft_pr_and_missing_workpad_evidence() {
    let config = test_config();
    let mut issue = tracker_issue("In Progress");
    issue
        .linked_pull_requests
        .push(jade_symphony::model::LinkedPullRequest {
            id: Some("PR_57".into()),
            number: Some(57),
            url: Some("https://github.com/Alive24/jade-symphony/pull/57".into()),
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
            pr_url: "https://github.com/Alive24/jade-symphony/pull/45".into(),
            was_draft: true,
            marked_ready: false,
        });
    }
    let draft_workpad = run_loop_handoff_workpad(&issue, &draft_result, &handoff, None);
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

#[test]
fn no_dispatch_sleeps_without_iteration_limit() {
    let options = RunLoopOptions {
        workflow_path: PathBuf::from("WORKFLOW.md"),
        max_iterations: None,
        once: false,
        max_concurrent: None,
        write: false,
        recover: false,
        display: DisplayMode::Plain,
    };

    assert_eq!(
        no_dispatch_action(options.iteration_limit(), 250),
        NoDispatchAction::SleepAndContinue { delay_ms: 250 }
    );
}

#[test]
fn run_loop_write_mode_rejects_dry_run_backend_before_runtime_writes() {
    let temp = tempfile::tempdir().unwrap();
    let workspace_root = temp.path().join("workspaces");
    let logs_root = temp.path().join("logs");
    let workflow_path = temp.path().join("WORKFLOW.md");
    std::fs::write(
            &workflow_path,
            format!(
                "---\ntracker:\n  kind: memory\nworkspace:\n  root: {}\nobservability:\n  logs_root: {}\nmain_lane:\n  backend: dry-run\n---\nPrompt",
                workspace_root.display(),
                logs_root.display()
            ),
        )
        .unwrap();

    let error = run_loop(RunLoopOptions {
        workflow_path: workflow_path.clone(),
        max_iterations: Some(1),
        once: false,
        max_concurrent: None,
        write: true,
        recover: false,
        display: DisplayMode::Plain,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("write-mode main loop is blocked"));
    assert!(error.contains("main_lane.backend=dry-run"));
    assert!(error.contains(workflow_path.to_string_lossy().as_ref()));
    assert!(
        !workspace_root.exists(),
        "guard must fire before workspace creation"
    );
    assert!(!logs_root.exists(), "guard must fire before runtime writes");
}

#[test]
fn run_loop_dry_run_preview_allows_dry_run_backend() {
    let temp = tempfile::tempdir().unwrap();
    let workspace_root = temp.path().join("workspaces");
    let logs_root = temp.path().join("logs");
    let workflow_path = temp.path().join("WORKFLOW.md");
    std::fs::write(
            &workflow_path,
            format!(
                "---\ntracker:\n  kind: memory\nworkspace:\n  root: {}\nobservability:\n  logs_root: {}\nmain_lane:\n  backend: dry-run\n---\nPrompt",
                workspace_root.display(),
                logs_root.display()
            ),
        )
        .unwrap();

    run_loop(RunLoopOptions {
        workflow_path,
        max_iterations: Some(1),
        once: false,
        max_concurrent: None,
        write: false,
        recover: false,
        display: DisplayMode::Plain,
    })
    .unwrap();
}

#[test]
fn no_dispatch_stops_for_bounded_write_loop() {
    let options = RunLoopOptions {
        workflow_path: PathBuf::from("WORKFLOW.md"),
        max_iterations: Some(2),
        once: false,
        max_concurrent: None,
        write: true,
        recover: false,
        display: DisplayMode::Plain,
    };

    assert_eq!(
        no_dispatch_action(options.iteration_limit(), 250),
        NoDispatchAction::Stop {
            reason: "no_dispatchable_issue"
        }
    );
}

#[test]
fn no_dispatch_sleeps_for_unbounded_write_loop() {
    let options = RunLoopOptions {
        workflow_path: PathBuf::from("WORKFLOW.md"),
        max_iterations: None,
        once: false,
        max_concurrent: None,
        write: true,
        recover: false,
        display: DisplayMode::Plain,
    };

    assert_eq!(
        no_dispatch_action(options.iteration_limit(), 250),
        NoDispatchAction::SleepAndContinue { delay_ms: 250 }
    );
}
