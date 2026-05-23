use std::collections::BTreeMap;
use std::path::PathBuf;

use jade_symphony::config::RuntimeConfig;
use jade_symphony::model::{native_subissue_gate_blocker, normalize_state, TrackerIssue};
use jade_symphony::presentation::render_project_state_panel;
use jade_symphony::progress::run_with_progress_heartbeat;
use jade_symphony::review::transition_allowed_for_main_agent;
use jade_symphony::review_status::{
    load_review_status, render_project_inspect_review_summary, ReviewStatusOptions,
};
use jade_symphony::session_registry::unix_timestamp_ms;
use jade_symphony::tracker::{
    adapter_from_config, classify_project_state_error, TrackerAdapter, TrackerError,
};
use jade_symphony::workflow::WorkflowDefinition;

use crate::cli::DisplayMode;
use crate::{
    add_timeline_comment_with_recovery, append_canonical_checkout_gap,
    append_tracker_mutation_audit, evaluate_issue_for_current_source, linked_pull_requests_contain,
    load_config, progress_spec_for_config, reconcile_main_handoff_runtime_state, recovery_key,
    report_canonical_checkout_readonly, require_write_intent, set_state_with_recovery,
    stable_recovery_hash, tracker_backend_label, upsert_workpad_with_recovery,
    warn_if_temporary_workflow_path, AgentSessionLaneArg, TrackerMutationAudit,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectStateOptions {
    pub(crate) workflow_path: PathBuf,
    pub(crate) display: DisplayMode,
}

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

pub(crate) fn project_state(
    options: ProjectStateOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let workflow_path = options.workflow_path;
    warn_if_temporary_workflow_path(&workflow_path);
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;

    let adapter = adapter_from_config(&config);
    let read_result = run_with_progress_heartbeat(
        progress_spec_for_config(&config, "github_project_read")
            .backend(tracker_backend_label(&config))
            .next("load_project_summary"),
        || adapter.list_project_summary_issues(),
    );
    match read_result {
        Ok(issues) => {
            let mut integration_gaps = adapter.integration_gaps();
            append_canonical_checkout_gap(&config, &mut integration_gaps);
            if options.display == DisplayMode::Tui {
                println!("{}", render_project_state_panel(&issues, &integration_gaps));
                return Ok(());
            }
            println!("project_state_access=ok");
            println!("trusted=true");
            println!("issues={}", issues.len());
            println!("empty_queue={}", issues.is_empty());
            println!("{}", render_state_summary(&issues));
            for line in report_canonical_checkout_readonly(&config) {
                println!("{line}");
            }
            for gap in integration_gaps {
                println!("integration_gap={gap}");
            }
            Ok(())
        }
        Err(error) => {
            let kind = classify_project_state_error(&error);
            println!("project_state_access=blocked");
            println!("trusted=false");
            println!("failure_kind={}", kind.as_str());
            println!("failure={error}");
            Err(format!(
                "project state access is not trustworthy: kind={} error={error}",
                kind.as_str()
            )
            .into())
        }
    }
}

pub(crate) fn project_issue(
    workflow_path: PathBuf,
    issue_ref: String,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let issue = run_with_progress_heartbeat(
        progress_spec_for_config(&config, "github_project_read")
            .issue(issue_ref.clone())
            .backend(tracker_backend_label(&config))
            .next("load_issue"),
        || adapter.get_issue(&issue_ref),
    )?
    .ok_or_else(|| format!("issue not found: {issue_ref}"))?;

    if json {
        println!("{}", serde_json::to_string_pretty(&issue)?);
        return Ok(());
    }

    println!("issue={}", issue.identifier);
    println!("title={}", issue.title);
    println!("state={}", issue.state);
    println!("tracker={}", issue.tracker_kind);
    if let Some(item_id) = &issue.item_id {
        println!("project_item={item_id}");
    }
    if !issue.assignees.is_empty() {
        println!("assignees={}", issue.assignees.join(","));
    }
    if !issue.blocked_by.is_empty() {
        let blockers = issue
            .blocked_by
            .iter()
            .map(|blocker| {
                blocker
                    .identifier
                    .as_deref()
                    .or(blocker.id.as_deref())
                    .unwrap_or("unknown")
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join(",");
        println!("blocked_by={blockers}");
    } else {
        println!("blocked_by=");
    }
    if !issue.linked_pull_requests.is_empty() {
        for pr in &issue.linked_pull_requests {
            let pr_ref = pr
                .url
                .clone()
                .or_else(|| pr.number.map(|number| format!("#{number}")))
                .unwrap_or_else(|| "unknown".into());
            println!(
                "linked_pr={} state={}",
                pr_ref,
                pr.state.as_deref().unwrap_or("unknown")
            );
        }
    }
    for (name, value) in &issue.project_fields {
        println!("field.{name}={}", compact_json_value(value));
    }
    Ok(())
}

pub(crate) fn project_inspect(
    workflow_path: PathBuf,
    issue_ref: String,
    lane: Option<AgentSessionLaneArg>,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let issue = adapter
        .get_issue(&issue_ref)?
        .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    let gate = evaluate_issue_for_current_source(&config, &issue)?;
    let terminal_states = config.terminal_state_set().into_iter().collect();
    let native_subissue_blocker = native_subissue_gate_blocker(&issue, &terminal_states);

    println!("project_inspect=ok");
    println!("read_only=true");
    println!("issue={}", issue.identifier);
    println!("title={}", issue.title);
    println!("state={}", issue.state);
    if let Some(lane) = lane {
        println!("lane={}", lane.label());
    }
    println!("gate={:?}", gate.kind);
    println!(
        "dispatchable={}",
        gate.is_dispatchable() && native_subissue_blocker.is_none()
    );
    if let Some(reason) = &native_subissue_blocker {
        println!("native_subissue_gate={reason}");
    }
    if !gate.missing.is_empty() {
        println!("missing={}", gate.missing.join(", "));
    }
    if !gate.assumptions.is_empty() {
        println!("assumptions={}", gate.assumptions.join("; "));
    }
    if issue.blocked_by.is_empty() {
        println!("blocked_by=");
    } else {
        let blockers = issue
            .blocked_by
            .iter()
            .map(|blocker| {
                blocker
                    .identifier
                    .as_deref()
                    .or(blocker.id.as_deref())
                    .unwrap_or("unknown")
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join(",");
        println!("blocked_by={blockers}");
    }
    if issue.linked_pull_requests.is_empty() {
        println!("linked_prs=");
    } else {
        for pr in &issue.linked_pull_requests {
            let pr_ref = pr
                .url
                .clone()
                .or_else(|| pr.number.map(|number| format!("#{number}")))
                .unwrap_or_else(|| "unknown".into());
            println!(
                "linked_pr={} state={}",
                pr_ref,
                pr.state.as_deref().unwrap_or("unknown")
            );
        }
    }
    let review_status_options = ReviewStatusOptions {
        issue_filter: Some(issue.identifier.clone()),
        recent_limit: 1,
        verbose: false,
    };
    if let Ok(payload) = load_review_status(
        &config,
        std::slice::from_ref(&issue),
        &review_status_options,
        unix_timestamp_ms(),
    ) {
        if let Some(summary) = render_project_inspect_review_summary(&payload) {
            println!("review_status_summary={summary}");
        }
    }
    for gap in adapter.integration_gaps() {
        println!("integration_gap={gap}");
    }

    Ok(())
}

fn compact_json_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::Null => "null".into(),
        _ => serde_json::to_string(value).unwrap_or_else(|_| "<unprintable>".into()),
    }
}

pub(crate) fn filter_issues_by_state(
    issues: Vec<TrackerIssue>,
    state_filters: &[String],
) -> Vec<TrackerIssue> {
    if state_filters.is_empty() {
        return issues;
    }

    let normalized_filters = state_filters
        .iter()
        .map(|state| normalize_state(state))
        .collect::<Vec<_>>();
    issues
        .into_iter()
        .filter(|issue| {
            let issue_state = issue.normalized_state();
            normalized_filters
                .iter()
                .any(|filter| filter == &issue_state)
        })
        .collect()
}

pub(crate) fn render_state_summary(issues: &[TrackerIssue]) -> String {
    let mut counts = BTreeMap::new();
    for issue in issues {
        let state = issue.state.trim();
        let state = if state.is_empty() { "(unknown)" } else { state };
        *counts.entry(state.to_string()).or_insert(0usize) += 1;
    }

    let summary = if counts.is_empty() {
        "(none)".to_string()
    } else {
        counts
            .into_iter()
            .map(|(state, count)| format!("{state}:{count}"))
            .collect::<Vec<_>>()
            .join(", ")
    };

    format!("state_summary={summary}")
}
