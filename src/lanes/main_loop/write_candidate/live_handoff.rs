use shea_symphony::config::RuntimeConfig;
use shea_symphony::git_handoff::{
    commit_issue_worktree_changes, ensure_pull_request_ready, publish_issue_pull_request,
    LiveWorktreeResult, ProcessHandoffCommandRunner,
};
use shea_symphony::handoff::IssueHandoffPlan;
use shea_symphony::model::{LatestStatus, TrackerIssue};
use shea_symphony::runtime_profile::RuntimeProfile;
use shea_symphony::tracker::TrackerAdapter;

use crate::orchestration::{
    append_tracker_mutation_audit, latest_status_for_issue, print_latest_status,
    TrackerMutationAudit,
};

use super::super::{
    apply_live_handoff_pr_link, run_handoff_verification_with_runtime_profile, HandoffVerification,
    IssueExecutionResult, RunLoopLiveHandoff,
};

pub(super) fn apply_live_handoff_steps(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    latest: &TrackerIssue,
    handoff: &IssueHandoffPlan,
    live_worktree: Option<LiveWorktreeResult>,
    runtime_profile: Option<&RuntimeProfile>,
    result: &mut IssueExecutionResult,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(worktree) = live_worktree {
        let runner = ProcessHandoffCommandRunner;
        if result.backend == "codex" {
            let commit_message = format!(
                "Implement {}: {}",
                latest.identifier,
                latest.title.replace(['\n', '\r'], " ")
            );
            match commit_issue_worktree_changes(handoff, &runner, &commit_message) {
                Ok(commit) => {
                    println!(
                        "run_loop_action=commit issue={} committed={} hash={}",
                        latest.identifier,
                        commit.committed,
                        commit.commit_hash.as_deref().unwrap_or("n/a")
                    );
                }
                Err(error) => {
                    result.success = false;
                    result.message = format!("handoff commit failed: {error}");
                }
            }
        }
        let verification = if result.success {
            run_handoff_verification_with_runtime_profile(
                &handoff.workspace_path,
                config,
                runtime_profile,
            )
        } else {
            HandoffVerification {
                success: false,
                summary: result.message.clone(),
            }
        };
        println!(
            "run_loop_action=verify issue={} success={} summary={}",
            latest.identifier, verification.success, verification.summary
        );
        print_latest_status(&latest_status_for_issue(
            config,
            latest,
            "main",
            if verification.success {
                "handoff"
            } else {
                "failed"
            },
            "verify",
            Some(if verification.success {
                "publish PR".into()
            } else {
                "record failure".into()
            }),
        ));
        result.handoff_verification = Some(verification.summary.clone());
        if verification.success {
            match publish_issue_pull_request(handoff, &runner) {
                Ok(publication) => {
                    println!(
                        "run_loop_action=pr issue={} url={} created={}",
                        latest.identifier, publication.pr_url, publication.pr_created
                    );
                    print_latest_status(&LatestStatus {
                        lane: "main".into(),
                        category: "handoff".into(),
                        action: "pr_ready".into(),
                        issue_identifier: Some(latest.identifier.clone()),
                        issue_title: Some(latest.title.clone()),
                        actor_label: Some(config.identity.actor_label.clone()),
                        workspace: Some(worktree.workspace_path.display().to_string()),
                        branch: Some(worktree.branch_name.clone()),
                        session_id: result.session_id.clone(),
                        next: Some("link PR".into()),
                    });
                    result.live_handoff = Some(RunLoopLiveHandoff {
                        worktree,
                        publication,
                        verification: verification.summary,
                        project_pr_link_verified: None,
                        pull_request_ready: None,
                    });
                }
                Err(error) => {
                    result.success = false;
                    result.message = format!("handoff publication failed: {error}");
                }
            }
        } else {
            result.success = false;
            result.message = format!("handoff verification failed: {}", verification.summary);
        }
    }
    if result.success {
        let linked = apply_live_handoff_pr_link(adapter, &latest.identifier, result);
        if linked {
            append_tracker_mutation_audit(
                config,
                TrackerMutationAudit {
                    command: "main loop",
                    mutation_type: "pr_link",
                    issue_ref: Some(&latest.identifier),
                    target: result
                        .live_handoff
                        .as_ref()
                        .map(|handoff| handoff.publication.pr_url.clone()),
                    from_state: Some(latest.state.clone()),
                    to_state: None,
                    reason: "live handoff PR link",
                },
            );
            println!(
                "run_loop_action=link_pr issue={} evidence=live_handoff",
                latest.identifier
            );
        }
    }
    if result.success {
        if let Some(handoff) = result.live_handoff.as_mut() {
            match ensure_pull_request_ready(
                &handoff.publication.pr_url,
                &ProcessHandoffCommandRunner,
                &handoff.worktree.workspace_path,
            ) {
                Ok(ready) => {
                    println!(
                        "run_loop_action=pr_ready issue={} url={} was_draft={} marked_ready={}",
                        latest.identifier, ready.pr_url, ready.was_draft, ready.marked_ready
                    );
                    handoff.pull_request_ready = Some(ready);
                }
                Err(error) => {
                    result.success = false;
                    result.message = format!("handoff PR ready check failed: {error}");
                    println!(
                        "run_loop_action=blocked issue={} reason=pr_ready_check_failed error={}",
                        latest.identifier, error
                    );
                }
            }
        }
    }
    Ok(())
}
