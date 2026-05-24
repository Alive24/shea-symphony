use std::path::PathBuf;

use shea_symphony::tracker::{adapter_from_config, FollowUpIssueInput};

use crate::orchestration::{
    append_tracker_mutation_audit, load_config, require_write_intent, TrackerMutationAudit,
};

pub(crate) fn create_follow_up(
    workflow_path: PathBuf,
    title: String,
    body_path: PathBuf,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    require_write_intent(write)?;
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let body = std::fs::read_to_string(&body_path)?;
    let issue_id = adapter.create_follow_up_issue(FollowUpIssueInput {
        title,
        body,
        assignees: Vec::new(),
        project_id: None,
        related_issue_ref: None,
        blocked_by_issue_ref: None,
    })?;
    append_tracker_mutation_audit(
        &config,
        TrackerMutationAudit {
            command: "create-follow-up",
            mutation_type: "issue_create",
            issue_ref: None,
            target: Some(issue_id.clone()),
            from_state: None,
            to_state: None,
            reason: "explicit CLI follow-up creation",
        },
    );
    println!("create_follow_up=ok issue_id={issue_id}");
    Ok(())
}
