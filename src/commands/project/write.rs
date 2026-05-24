use std::path::PathBuf;

use jade_symphony::model::normalize_state;
use jade_symphony::review::transition_allowed_for_main_agent;
use jade_symphony::tracker::{adapter_from_config, TrackerAdapter, TrackerError};

use crate::{
    add_timeline_comment_with_recovery, append_tracker_mutation_audit,
    linked_pull_requests_contain, load_config, reconcile_main_handoff_runtime_state, recovery_key,
    require_write_intent, set_state_with_recovery, stable_recovery_hash,
    upsert_workpad_with_recovery, TrackerMutationAudit,
};

pub(crate) fn set_state(
    workflow_path: PathBuf,
    issue_ref: String,
    state: String,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    require_write_intent(write)?;
    if !transition_allowed_for_main_agent(&normalize_state(&state)) {
        return Err("main implementation agent cannot set Human Review".into());
    }
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let initial_issue = adapter.get_issue(&issue_ref)?;
    let from_state = initial_issue
        .as_ref()
        .map(|issue| issue.state.clone())
        .filter(|current| !current.is_empty());
    let outcome = set_state_with_recovery(
        adapter.as_ref(),
        &issue_ref,
        initial_issue.as_ref(),
        &state,
        "state_change",
    )?;
    reconcile_main_handoff_runtime_state(&config, &issue_ref, &state)?;
    if outcome.should_record_audit() {
        append_tracker_mutation_audit(
            &config,
            TrackerMutationAudit {
                command: "set-state",
                mutation_type: "state_change",
                issue_ref: Some(&issue_ref),
                target: None,
                from_state,
                to_state: Some(state.clone()),
                reason: "explicit CLI state update",
            },
        );
    }
    println!(
        "set_state={} issue_ref={issue_ref} state={state}",
        outcome.as_str()
    );
    Ok(())
}

pub(crate) fn upsert_workpad(
    workflow_path: PathBuf,
    issue_ref: String,
    markdown_path: PathBuf,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    require_write_intent(write)?;
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let markdown = std::fs::read_to_string(&markdown_path)?;
    let initial_issue = adapter.get_issue(&issue_ref)?;
    let key = recovery_key(
        "workpad",
        &issue_ref,
        &format!(
            "{}|{}",
            markdown_path.display(),
            stable_recovery_hash(&markdown)
        ),
    );
    let outcome = upsert_workpad_with_recovery(
        adapter.as_ref(),
        &issue_ref,
        initial_issue.as_ref(),
        &markdown,
        &key,
    )?;
    if outcome.should_record_audit() {
        append_tracker_mutation_audit(
            &config,
            TrackerMutationAudit {
                command: "workpad",
                mutation_type: "workpad_write",
                issue_ref: Some(&issue_ref),
                target: Some(markdown_path.display().to_string()),
                from_state: None,
                to_state: None,
                reason: "explicit CLI workpad upsert",
            },
        );
    }
    println!(
        "workpad={} issue_ref={} source={}",
        outcome.as_str(),
        issue_ref,
        markdown_path.display()
    );
    Ok(())
}

pub(crate) fn append_timeline_comment(
    workflow_path: PathBuf,
    issue_ref: String,
    markdown_path: PathBuf,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let markdown = std::fs::read_to_string(&markdown_path)?;
    if !write {
        println!(
            "timeline_comment_dry_run action=add_issue_comment issue_ref={} source={}",
            issue_ref,
            markdown_path.display()
        );
        return Ok(());
    }

    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let initial_issue = adapter.get_issue(&issue_ref)?;
    let from_state = initial_issue
        .as_ref()
        .map(|issue| issue.state.clone())
        .filter(|current| !current.is_empty());
    let key = recovery_key(
        "timeline-comment",
        &issue_ref,
        &format!(
            "{}|{}",
            markdown_path.display(),
            stable_recovery_hash(&markdown)
        ),
    );
    let outcome = add_timeline_comment_with_recovery(
        adapter.as_ref(),
        &issue_ref,
        initial_issue.as_ref(),
        &markdown,
        &key,
        "timeline_comment",
    )?;
    if outcome.should_record_audit() {
        append_tracker_mutation_audit(
            &config,
            TrackerMutationAudit {
                command: "timeline-comment",
                mutation_type: "timeline_comment",
                issue_ref: Some(&issue_ref),
                target: Some(markdown_path.display().to_string()),
                from_state,
                to_state: None,
                reason: "explicit CLI append-only timeline comment",
            },
        );
    }
    println!(
        "timeline_comment={} issue_ref={} source={}",
        outcome.as_str(),
        issue_ref,
        markdown_path.display()
    );
    Ok(())
}

pub(crate) fn link_pr(
    workflow_path: PathBuf,
    issue_ref: String,
    pr_ref: String,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if !write {
        println!("link_pr_dry_run action=link_pull_request issue_ref={issue_ref} pr_ref={pr_ref}");
        return Ok(());
    }

    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let repaired = link_pr_with_adapter(adapter.as_ref(), &issue_ref, &pr_ref, true)?;
    if repaired {
        append_tracker_mutation_audit(
            &config,
            TrackerMutationAudit {
                command: "link-pr",
                mutation_type: "pr_link",
                issue_ref: Some(&issue_ref),
                target: Some(pr_ref.clone()),
                from_state: None,
                to_state: None,
                reason: "explicit CLI PR link",
            },
        );
    }
    let action = if repaired {
        "repair_comment"
    } else {
        "already_visible"
    };
    println!("link_pr=ok issue_ref={issue_ref} pr_ref={pr_ref} action={action}");
    Ok(())
}

pub(crate) fn link_pr_with_adapter(
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    pr_ref: &str,
    write: bool,
) -> Result<bool, TrackerError> {
    if write {
        let linked = adapter.list_linked_pull_requests(issue_ref)?;
        if linked_pull_requests_contain(&linked, pr_ref) {
            return Ok(false);
        }
        adapter.link_pull_request(issue_ref, pr_ref)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

pub(crate) fn add_to_project(
    workflow_path: PathBuf,
    issue_id: String,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    require_write_intent(write)?;
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    adapter.add_issue_to_project(&issue_id)?;
    append_tracker_mutation_audit(
        &config,
        TrackerMutationAudit {
            command: "add-to-project",
            mutation_type: "project_add",
            issue_ref: Some(&issue_id),
            target: Some("Project item".into()),
            from_state: None,
            to_state: Some("todo".into()),
            reason: "explicit CLI project add",
        },
    );
    println!("add_to_project=ok issue_id={issue_id}");
    Ok(())
}
