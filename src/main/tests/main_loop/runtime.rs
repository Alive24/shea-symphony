use super::*;

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
        &shea_symphony::session_registry::SessionRegistry {
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
    state.backend_session_id = Some("shea-main-missing".into());

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
    state.backend_session_id = Some("shea-main-29-attempt-1".into());
    state.updated_at_ms = Some(1_000);

    let mut record = main_tmux_session_record("#29", SessionStatus::Running);
    record.session_name = "shea-main-29-attempt-1".into();
    record.pane_target = "shea-main-29-attempt-1".into();
    record.log_path = temp.path().join("session.log");
    record.updated_at_ms = 1_000;
    save_session_registry(
        &session_registry_path(&config),
        &shea_symphony::session_registry::SessionRegistry {
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
    record.session_name = "shea-main-29-attempt-1".into();
    record.pane_target = "shea-main-29-attempt-1".into();
    record.log_path = temp.path().join("session.log");
    record.updated_at_ms = 1_000;
    save_session_registry(
        &session_registry_path(&config),
        &shea_symphony::session_registry::SessionRegistry {
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
        &shea_symphony::session_registry::SessionRegistry {
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
fn resume_preflight_many_terminates_stale_codex_app_server_process_before_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    config.codex.session_stale_after_ms = 1_000;
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut child = ProcessCommand::new("sleep").arg("30").spawn().unwrap();
    let mut record = main_tmux_session_record("#29", SessionStatus::Running);
    record.backend = "codex".into();
    record.session_source = Some("codex-app-server".into());
    record.session_name = "thread-29-turn-stale".into();
    record.process_id = Some(child.id());
    record.pane_target = String::new();
    record.attach_command =
        "not a tmux session; inspect app-server artifacts for recovery evidence".into();
    record.log_path = temp.path().join("logs/app-server/29.events.json");
    record.updated_at_ms = 1_000;
    save_session_registry(
        &session_registry_path(&config),
        &shea_symphony::session_registry::SessionRegistry {
            sessions: vec![record],
        },
    )
    .unwrap();

    let summary = run_loop_resume_preflight_many(&tracker, &config, &[], 3_000, true).unwrap();

    assert_eq!(summary.active_main_workers, 0);
    assert_eq!(summary.blocked, None);
    assert_eq!(summary.recoverable_states.len(), 1);
    assert!(summary.recoverable_states[0]
        .reason
        .contains("terminated_process_id="));
    let mut exited = false;
    for _ in 0..20 {
        if child.try_wait().unwrap().is_some() {
            exited = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    if !exited {
        let _ = child.kill();
        let _ = child.wait();
    }
    assert!(exited, "stale app-server process should be terminated");
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
    record.session_name = "shea-main-29-attempt-1".into();
    record.pane_target = "shea-main-29-attempt-1".into();
    record.log_path = temp.path().join("session.log");
    save_session_registry(
        &session_registry_path(&config),
        &shea_symphony::session_registry::SessionRegistry {
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
    record.session_name = "shea-main-29-attempt-1".into();
    record.pane_target = "shea-main-29-attempt-1".into();
    record.log_path = temp.path().join("session.log");
    save_session_registry(
        &session_registry_path(&config),
        &shea_symphony::session_registry::SessionRegistry {
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
    attempt_two.session_name = "shea-main-29-attempt-2".into();
    attempt_two.pane_target = "shea-main-29-attempt-2".into();
    attempt_two.log_path = temp.path().join("attempt-2.log");
    attempt_two.attempt = 2;
    let mut attempt_three = main_tmux_session_record("#29", SessionStatus::Running);
    attempt_three.session_name = "shea-main-29-attempt-3".into();
    attempt_three.pane_target = "shea-main-29-attempt-3".into();
    attempt_three.log_path = temp.path().join("attempt-3.log");
    attempt_three.attempt = 3;
    save_session_registry(
        &session_registry_path(&config),
        &shea_symphony::session_registry::SessionRegistry {
            sessions: vec![attempt_two, attempt_three],
        },
    )
    .unwrap();
    let mut state = active_runtime_state("#29");
    state.backend = "tmux".into();
    state.last_event = Some("SessionRunning".into());
    state.backend_session_id = Some("shea-main-29-attempt-3".into());
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
        Some("shea-main-29-attempt-2")
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
    record.session_name = "shea-main-29-attempt-1".into();
    record.pane_target = "shea-main-29-attempt-1".into();
    record.log_path = temp.path().join("session.log");
    save_session_registry(
        &session_registry_path(&config),
        &shea_symphony::session_registry::SessionRegistry {
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
    record.session_name = "shea-main-29-attempt-1".into();
    record.pane_target = "shea-main-29-attempt-1".into();
    record.log_path = temp.path().join("session.log");
    save_session_registry(
        &session_registry_path(&config),
        &shea_symphony::session_registry::SessionRegistry {
            sessions: vec![record],
        },
    )
    .unwrap();
    let mut state = active_runtime_state("#29");
    state.backend = "tmux".into();
    state.last_event = Some("SessionRunning".into());
    state.backend_session_id = Some("shea-main-29-attempt-1".into());

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
