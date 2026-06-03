use super::*;

#[test]
fn merge_session_defaults_to_codex_app_server_command() {
    let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\ncodex:\n  command: /opt/homebrew/bin/codex app-server\n---\nPrompt",
        )
        .unwrap();
    let config = RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

    let spec = agent_session_backend_spec(&config, AgentSessionLaneArg::Merge).unwrap();

    assert_eq!(spec.backend, "codex");
    assert_eq!(spec.command, "/opt/homebrew/bin/codex app-server");
}

#[test]
fn merge_session_keeps_tmux_as_explicit_fallback() {
    let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nmerge_lane:\n  agent_backend: tmux\ntmux:\n  agent_command: codex\n  merge_agent_command: codex --profile merge\n---\nPrompt",
        )
        .unwrap();
    let config = RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

    let spec = agent_session_backend_spec(&config, AgentSessionLaneArg::Merge).unwrap();

    assert_eq!(spec.backend, "tmux");
    assert_eq!(spec.command, "codex --profile merge");
}

#[test]
fn clean_merge_tick_does_not_require_merge_agent_backend() {
    let temp = tempfile::tempdir().unwrap();
    let workflow_path = temp.path().join("WORKFLOW.md");
    std::fs::write(
            &workflow_path,
            "---\ntracker:\n  kind: memory\nmerge_lane:\n  agent_backend: definitely-not-a-session-backend\n---\nPrompt",
        )
        .unwrap();

    let outcome = merge_once_tick(workflow_path, false, false, false).unwrap();

    assert_eq!(outcome, MergeOnceOutcome::NoMergingIssue);
}

#[test]
fn successful_merge_agent_repair_records_merging_retry_rationale() {
    let config = test_config();
    let issue = tracker_issue_with_ref("#390", "Route DIRTY repair", "Merging");
    let runner = MergeRecoveryRunner::new();

    let outcome = finish_merge_agent_repaired_branch(
        &config,
        &issue,
        "merge_agent",
        "src/main.rs conflicted",
        "resolved conflict by preserving approved behavior",
        "approved implementation intent preserved",
        vec![
            "git diff --name-only --diff-filter=U".into(),
            "git diff --check".into(),
            "git status --porcelain".into(),
        ],
        "https://github.com/Alive24/shea-symphony/pull/390",
        "feature/issue-390",
        &runner,
        Path::new("."),
        CommandOutput {
            status: 0,
            stdout: "MERGE_AGENT_DECISION: repaired".into(),
            stderr: String::new(),
        },
        "codex".into(),
        Some("session-390".into()),
    )
    .unwrap();

    assert!(outcome.repaired);
    assert_eq!(outcome.evidence.method, "merge_agent");
    assert!(outcome
        .evidence
        .next_state_rationale
        .contains("stays in `Merging`"));
    assert!(runner
        .calls
        .borrow()
        .iter()
        .any(|call| call == "git push origin feature/issue-390"));
}

#[test]
fn dirty_repair_aborts_interrupted_merge_state_before_retrying() {
    let runner = InterruptedMergeRepairRunner::new();

    let outcome = repair_dirty_pull_request(
        "https://github.com/Alive24/shea-symphony/pull/390",
        Some("feature/issue-390"),
        "main",
        &runner,
        Path::new("."),
        false,
    )
    .unwrap();

    assert!(outcome.repaired);
    assert_eq!(outcome.failure_kind, None);
    let calls = runner.calls.borrow();
    assert!(calls.iter().any(|call| call == "git merge --abort"));
    assert!(calls
        .iter()
        .any(|call| call == "git merge --no-edit origin/main"));
    assert!(calls
        .iter()
        .any(|call| call == "git push origin feature/issue-390"));
}

#[test]
fn merge_agent_semantic_uncertainty_marker_requires_human_input() {
    let text = "\
RESOLUTION_SUMMARY: conflict needs product choice
SEMANTIC_SAFETY: cannot prove reviewed intent
MERGE_AGENT_DECISION: needs_human_input";

    assert!(merge_agent_requests_human_input(text));
    assert!(!merge_agent_reports_repaired(text));
}

struct InterruptedMergeRepairRunner {
    calls: std::cell::RefCell<Vec<String>>,
    aborted: std::cell::RefCell<bool>,
}

impl InterruptedMergeRepairRunner {
    fn new() -> Self {
        Self {
            calls: std::cell::RefCell::new(Vec::new()),
            aborted: std::cell::RefCell::new(false),
        }
    }
}

impl HandoffCommandRunner for InterruptedMergeRepairRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
        _cwd: &Path,
    ) -> Result<CommandOutput, shea_symphony::git_handoff::GitHandoffError> {
        let command = format!("{program} {}", args.join(" "));
        self.calls.borrow_mut().push(command.clone());
        let aborted = *self.aborted.borrow();
        match command.as_str() {
            "git worktree list --porcelain" => Ok(CommandOutput {
                status: 0,
                stdout:
                    "worktree /tmp/issue-390\nHEAD abc123\nbranch refs/heads/feature/issue-390\n\n"
                        .into(),
                stderr: String::new(),
            }),
            "git status --porcelain" if !aborted => Ok(CommandOutput {
                status: 0,
                stdout: "UU app/test/operator-view.test.mjs\n".into(),
                stderr: String::new(),
            }),
            "git status --porcelain" => Ok(CommandOutput {
                status: 0,
                stdout: String::new(),
                stderr: String::new(),
            }),
            "git rev-parse -q --verify MERGE_HEAD" if !aborted => Ok(CommandOutput {
                status: 0,
                stdout: "abc123\n".into(),
                stderr: String::new(),
            }),
            "git rev-parse -q --verify MERGE_HEAD" => Ok(CommandOutput {
                status: 1,
                stdout: String::new(),
                stderr: String::new(),
            }),
            "git diff --name-only --diff-filter=U" if !aborted => Ok(CommandOutput {
                status: 0,
                stdout: "app/test/operator-view.test.mjs\n".into(),
                stderr: String::new(),
            }),
            "git diff --name-only --diff-filter=U" => Ok(CommandOutput {
                status: 0,
                stdout: String::new(),
                stderr: String::new(),
            }),
            "git merge --abort" => {
                *self.aborted.borrow_mut() = true;
                Ok(CommandOutput {
                    status: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                })
            }
            "git fetch origin main" | "git merge --no-edit origin/main" => Ok(CommandOutput {
                status: 0,
                stdout: String::new(),
                stderr: String::new(),
            }),
            "git push origin feature/issue-390" => Ok(CommandOutput {
                status: 0,
                stdout: "pushed\n".into(),
                stderr: String::new(),
            }),
            _ => Ok(CommandOutput {
                status: 0,
                stdout: String::new(),
                stderr: String::new(),
            }),
        }
    }
}

#[test]
fn merge_pool_selection_only_accepts_merging_lane() {
    let config = test_config();
    let mut claimed = tracker_issue_with_ref("#6", "Claimed merge", "Merging");
    claimed.project_fields.insert(
        "Merging Agent".into(),
        serde_json::Value::String("other merger".into()),
    );
    let mut unclaimed = tracker_issue_with_ref("#7", "Ready merge", "Merging");
    unclaimed.priority = Some(1);
    let todo = tracker_issue_with_ref("#8", "Main work", "Todo");

    let selected = select_pool_worker_issues(
        &[claimed, unclaimed, todo],
        WorkerLane::Merging,
        "this merger",
        4,
        &config,
    );

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].identifier, "#7");
}

#[test]
fn merge_pool_selection_reuses_structured_active_claim_for_same_worker() {
    let config = test_config();
    let worker = "Shea Symphony Agent";
    let claim = LaneClaim::active(
        "#6",
        LaneClaimLane::Merge,
        LaneClaimActor::Codex,
        LaneClaimSource::Loop,
        1_779_000_000_000,
    )
    .with_worker(worker);
    let mut claimed_by_self = tracker_issue_with_ref("#6", "Claimed merge", "Merging");
    claimed_by_self.project_fields.insert(
        "Merging Agent".into(),
        serde_json::Value::String(claim.render()),
    );

    let selected =
        select_pool_worker_issues(&[claimed_by_self], WorkerLane::Merging, worker, 1, &config);

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].identifier, "#6");
}

#[test]
fn merge_recover_selection_prioritizes_interrupted_loop_claims() {
    let config = test_config();
    let worker = "Shea Symphony Agent";
    let interrupted_claim = LaneClaim::active(
        "#6",
        LaneClaimLane::Merge,
        LaneClaimActor::Codex,
        LaneClaimSource::Loop,
        1_779_000_000_000,
    )
    .with_worker("previous merger");
    let mut interrupted = tracker_issue_with_ref("#6", "Interrupted merge", "Merging");
    interrupted.priority = Some(20);
    interrupted.project_fields.insert(
        "Merging Agent".into(),
        serde_json::Value::String(interrupted_claim.render()),
    );
    let mut unclaimed = tracker_issue_with_ref("#7", "Ready merge", "Merging");
    unclaimed.priority = Some(1);

    let selected = select_merge_worker_issues(&[unclaimed, interrupted], worker, 1, &config, true);

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].issue.identifier, "#6");
    assert!(selected[0]
        .recovery_reason
        .as_deref()
        .unwrap()
        .contains("previous_worker=previous merger"));
}

#[test]
fn merge_recover_selection_does_not_adopt_manual_claims() {
    let config = test_config();
    let worker = "Shea Symphony Agent";
    let manual_claim = LaneClaim::active(
        "#6",
        LaneClaimLane::Merge,
        LaneClaimActor::Codex,
        LaneClaimSource::Manual,
        1_779_000_000_000,
    )
    .with_worker("manual merger");
    let mut manual = tracker_issue_with_ref("#6", "Manual merge", "Merging");
    manual.project_fields.insert(
        "Merging Agent".into(),
        serde_json::Value::String(manual_claim.render()),
    );
    let unclaimed = tracker_issue_with_ref("#7", "Ready merge", "Merging");

    let selected = select_merge_worker_issues(&[manual, unclaimed], worker, 2, &config, true);

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].issue.identifier, "#7");
    assert!(selected[0].recovery_reason.is_none());
}

#[test]
fn merge_completion_closes_issue_after_workpad_and_done_state() {
    let adapter = RecordingAdapter::default();
    let issue = tracker_issue("Merging");
    let claim = LaneClaim::active(
        &issue.identifier,
        LaneClaimLane::Merge,
        LaneClaimActor::Codex,
        LaneClaimSource::Loop,
        1_779_000_000_000,
    )
    .with_worker("Shea Symphony Agent");
    let workpad = "## Shea Symphony Merge Run\n\n### Merge Action\n";

    let config = test_config();
    record_done_merge_lane_completion(&config, &adapter, &issue, &claim, workpad).unwrap();

    assert_eq!(
        adapter.operations(),
        vec![
            "comment:#29".to_string(),
            format!(
                "set_project_field:#29:Merging Agent={}",
                terminal_lane_claim_value(&claim, LaneClaimState::Done, "merged").unwrap()
            ),
            "set_state:#29:done".to_string(),
            "close_issue:#29".to_string()
        ]
    );
}

#[test]
fn merge_completion_preserves_terminal_claim_audit_pointer() {
    let adapter = RecordingAdapter::default();
    let issue = tracker_issue("Merging");
    adapter
        .issues
        .borrow_mut()
        .insert(issue.identifier.clone(), issue.clone());
    let claim = LaneClaim::active(
        &issue.identifier,
        LaneClaimLane::Merge,
        LaneClaimActor::Codex,
        LaneClaimSource::Loop,
        1_779_000_000_000,
    )
    .with_worker("Shea Symphony Agent");

    record_done_merge_lane_completion(
        &test_config(),
        &adapter,
        &issue,
        &claim,
        "## Shea Symphony Merge Run\n\n### Merge Action\n",
    )
    .unwrap();

    let stored_issue = adapter
        .issues
        .borrow()
        .get(&issue.identifier)
        .unwrap()
        .clone();
    let stored_claim = project_text_field(&stored_issue, "Merging Agent").unwrap();
    assert!(stored_claim.contains("state=done"));
    assert!(stored_claim.contains("result=merged"));
    assert_eq!(
        LaneClaim::parse(&stored_claim).unwrap(),
        claim.with_state(LaneClaimState::Done)
    );
}
