use std::fs;
use std::path::{Path, PathBuf};

use jade_symphony::artifacts::{
    artifact_layout, cleanup_plan, ArtifactClass, CleanupCandidate, CleanupPlan,
};
use jade_symphony::canonical_checkout::canonical_quarantine_root;
use jade_symphony::session_registry::session_registry_path;
use jade_symphony::tracker::adapter_from_config;

use crate::orchestration::{load_config, session_status_snapshots};

pub(crate) fn cleanup_plan_command(
    workflow_path: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let states = config.tracker.terminal_states.clone();
    let issues = adapter.fetch_issues_by_states(&states)?;
    let layout = artifact_layout(&config);
    let plan = cleanup_plan(&config, &issues);

    println!("cleanup_plan=dry_run");
    println!("artifact_root={}", layout.root.display());
    println!("artifact_namespace={}", layout.namespace);
    println!("artifact_profile={}", layout.profile_namespace);
    println!(
        "artifact_class=per_issue_worktree path={}",
        layout.class_path(ArtifactClass::PerIssueWorktree).display()
    );
    println!(
        "artifact_class=runtime_state path={}",
        layout.class_path(ArtifactClass::RuntimeState).display()
    );
    println!(
        "artifact_class=event_log path={}",
        layout.class_path(ArtifactClass::EventLog).display()
    );
    println!(
        "artifact_class=rendered_agent_prompt path={}",
        layout
            .class_path(ArtifactClass::RenderedAgentPrompt)
            .join("prompts")
            .display()
    );
    println!(
        "artifact_class=review_job_artifact path={}",
        layout
            .class_path(ArtifactClass::ReviewJobArtifact)
            .display()
    );
    println!(
        "artifact_class=pr_body_draft path={}",
        layout
            .class_path(ArtifactClass::PullRequestBodyDraft)
            .display()
    );
    println!(
        "artifact_class=workpad_draft path={}",
        layout.class_path(ArtifactClass::WorkpadDraft).display()
    );
    println!(
        "artifact_class=reusable_workflow_prompt path={}",
        layout
            .class_path(ArtifactClass::ReusableWorkflowPrompt)
            .display()
    );
    println!(
        "artifact_class=disposable_scratch path={}",
        layout
            .class_path(ArtifactClass::DisposableScratch)
            .display()
    );
    println!("{}", render_cleanup_plan(&plan));
    println!("cleanup_plan_write_supported=false");
    Ok(())
}

fn render_cleanup_plan(plan: &CleanupPlan) -> String {
    let mut lines = vec![
        format!("workspace_root={}", plan.workspace_root.display()),
        format!("cleanup_candidates={}", plan.candidates.len()),
    ];

    for candidate in &plan.candidates {
        lines.push(format!(
            "- issue={} state={} removable={} path={}",
            candidate.issue_identifier,
            candidate.issue_state,
            candidate.removable,
            candidate.path.display()
        ));
        lines.push(format!(
            "  branch={}",
            candidate.branch.as_deref().unwrap_or("unknown")
        ));
        lines.push(format!(
            "  linked_pr_state={}",
            candidate.linked_pr_state.as_deref().unwrap_or("none")
        ));
        for reason in &candidate.reasons {
            lines.push(format!("  reason={reason}"));
        }
        for blocker in &candidate.blockers {
            lines.push(format!("  blocker={blocker}"));
        }
    }

    lines.join("\n")
}

pub(crate) fn clean_audit_command(
    workflow_path: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let terminal_issues = adapter.fetch_issues_by_states(&config.tracker.terminal_states)?;
    let layout = artifact_layout(&config);
    let plan = cleanup_plan(&config, &terminal_issues);
    let sessions = session_status_snapshots(&config).unwrap_or_else(|error| {
        println!("clean_audit_warning kind=tmux_session_status reason={error}");
        Vec::new()
    });

    println!("clean_audit=read_only");
    println!("artifact_root={}", layout.root.display());
    println!("workspace_root={}", config.workspace.root.display());
    print_clean_audit_path(
        "safe_to_keep",
        "session_registry",
        session_registry_path(&config),
        "durable tmux session evidence until session state is reconciled",
    );
    print_clean_audit_path(
        "safe_to_keep",
        "runtime_state",
        layout.class_path(ArtifactClass::RuntimeState),
        "resume-critical while an issue is active",
    );
    print_clean_audit_path(
        "safe_to_keep",
        "event_log",
        layout.class_path(ArtifactClass::EventLog),
        "local execution evidence",
    );
    print_clean_audit_path(
        "attach_to_tracker",
        "rendered_agent_prompt",
        config.observability.logs_root.join("prompts"),
        "prompt artifacts should stay available until tracker evidence names the run",
    );
    print_clean_audit_path(
        "safe_to_keep",
        "tmux_log",
        config.observability.logs_root.join("tmux"),
        "tmux logs are operator recovery evidence for interrupted sessions",
    );
    print_clean_audit_path(
        "safe_to_keep",
        "review_job_artifact",
        layout.class_path(ArtifactClass::ReviewJobArtifact),
        "review evidence until tracker workpad records it",
    );
    print_clean_audit_path(
        "attach_to_tracker",
        "pr_body_draft",
        layout.class_path(ArtifactClass::PullRequestBodyDraft),
        "draft should be represented by a pull request or issue workpad",
    );
    print_clean_audit_path(
        "attach_to_tracker",
        "workpad_draft",
        layout.class_path(ArtifactClass::WorkpadDraft),
        "draft should be represented by tracker-visible evidence",
    );
    print_clean_audit_path(
        "promote_to_repo",
        "reusable_workflow_prompt",
        layout.class_path(ArtifactClass::ReusableWorkflowPrompt),
        "workflow and prompt material should live in repo docs, examples, or workflows",
    );
    print_clean_audit_path(
        "cleanup_candidate",
        "disposable_scratch",
        layout.class_path(ArtifactClass::DisposableScratch),
        "scratch files are disposable after operator review",
    );
    print_clean_audit_path(
        "needs_human_decision",
        "canonical_checkout_quarantine",
        canonical_quarantine_root(&config),
        "files moved out of the canonical checkout before live write lanes should be archived or deleted after tracker evidence is settled",
    );

    let mut cleanup_candidates = 0;
    let mut human_decisions = 0;
    for candidate in &plan.candidates {
        if !candidate.path.exists() {
            continue;
        }
        if candidate.removable {
            cleanup_candidates += 1;
            println!(
                "clean_audit_item category=cleanup_candidate kind=worktree issue={} path={} reason=terminal_issue_clean_merged_or_closed",
                candidate.issue_identifier,
                candidate.path.display()
            );
        } else {
            human_decisions += 1;
            println!(
                "clean_audit_item category=needs_human_decision kind=worktree issue={} path={} reason={}",
                candidate.issue_identifier,
                candidate.path.display(),
                clean_audit_blocker_summary(candidate)
            );
        }
    }
    for session in &sessions {
        if session.status == "completed" {
            cleanup_candidates += 1;
            println!(
                "clean_audit_item category=cleanup_candidate kind=tmux_session issue={} session={} log={} prompt=unknown reason=session_completed_and_registry_evidence_present",
                session.issue_identifier.as_deref().unwrap_or("n/a"),
                session.session_id,
                session.log_path.as_deref().unwrap_or("n/a")
            );
        } else {
            human_decisions += 1;
            println!(
                "clean_audit_item category=needs_human_decision kind=tmux_session issue={} session={} status={} attach={} log={} reason=session_not_completed",
                session.issue_identifier.as_deref().unwrap_or("n/a"),
                session.session_id,
                session.status,
                session.attach_command.as_deref().unwrap_or("n/a"),
                session.log_path.as_deref().unwrap_or("n/a")
            );
        }
    }
    println!(
        "clean_audit_summary cleanup_candidates={cleanup_candidates} needs_human_decision={human_decisions}"
    );
    println!("clean_audit_write_supported=false");
    Ok(())
}

fn print_clean_audit_path(category: &str, kind: &str, path: impl AsRef<Path>, reason: &str) {
    let path = path.as_ref();
    let entries = read_dir_entry_count(path);
    println!(
        "clean_audit_item category={category} kind={kind} path={} exists={} entries={entries} reason={reason}",
        path.display(),
        path.exists()
    );
}

fn read_dir_entry_count(path: &Path) -> usize {
    fs::read_dir(path)
        .map(|entries| entries.filter_map(Result::ok).count())
        .unwrap_or(0)
}

fn clean_audit_blocker_summary(candidate: &CleanupCandidate) -> String {
    if !candidate.blockers.is_empty() {
        return candidate.blockers.join(",");
    }
    if !candidate.reasons.is_empty() {
        return candidate.reasons.join(",");
    }
    "operator_review_required".into()
}
