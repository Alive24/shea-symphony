use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde_json::{json, Value};
use shea_symphony::config::RuntimeConfig;
use shea_symphony::model::{
    native_subissue_gate_blocker, native_subissue_statuses, normalize_state, TrackerIssue,
};
use shea_symphony::presentation::render_project_state_panel;
use shea_symphony::progress::run_with_progress_heartbeat;
use shea_symphony::review_status::{
    load_review_status, render_project_inspect_review_summary, ReviewStatusOptions,
};
use shea_symphony::session_registry::unix_timestamp_ms;
use shea_symphony::tracker::{
    adapter_from_config, classify_project_state_error, IssueRelationshipReadback, TrackerAdapter,
};
use shea_symphony::workflow::WorkflowDefinition;

use crate::cli::DisplayMode;
use crate::commands::gate::evaluate_issue_for_current_source;
use crate::commands::session::AgentSessionLaneArg;
use crate::orchestration::{
    append_canonical_checkout_gap, append_tracker_mutation_audit, load_config,
    progress_spec_for_config, report_canonical_checkout_readonly, require_write_intent,
    tracker_backend_label, warn_if_temporary_workflow_path, TrackerMutationAudit,
};

mod write;

#[cfg(test)]
pub(crate) use write::link_pr_with_adapter;
pub(crate) use write::{
    add_to_project, append_timeline_comment, link_pr, set_state, upsert_workpad,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectStateOptions {
    pub(crate) workflow_path: PathBuf,
    pub(crate) display: DisplayMode,
    pub(crate) json: bool,
    pub(crate) include_terminal: bool,
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
            let issues = project_state_issues_for_scope(issues, &config, options.include_terminal);
            let mut integration_gaps = adapter.integration_gaps();
            append_canonical_checkout_gap(&config, &mut integration_gaps);
            if options.json {
                println!(
                    "{}",
                    render_project_state_json(
                        &issues,
                        &integration_gaps,
                        project_state_scope(options.include_terminal),
                        &config.terminal_state_set().into_iter().collect()
                    )?
                );
                return Ok(());
            }
            if options.display == DisplayMode::Tui {
                println!("{}", render_project_state_panel(&issues, &integration_gaps));
                return Ok(());
            }
            println!("project_state_access=ok");
            println!("trusted=true");
            println!("scope={}", project_state_scope(options.include_terminal));
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

pub(crate) fn project_state_issues_for_scope(
    issues: Vec<TrackerIssue>,
    config: &RuntimeConfig,
    include_terminal: bool,
) -> Vec<TrackerIssue> {
    if include_terminal {
        return issues;
    }
    let terminal_states = config.terminal_state_set();
    issues
        .into_iter()
        .filter(|issue| !terminal_states.contains(&issue.normalized_state()))
        .collect()
}

fn project_state_scope(include_terminal: bool) -> &'static str {
    if include_terminal {
        "all"
    } else {
        "queue"
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
                "linked_pr={} state={} source={}",
                pr_ref,
                pr.state.as_deref().unwrap_or("unknown"),
                linked_pull_request_source_label(pr)
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
                "linked_pr={} state={} source={}",
                pr_ref,
                pr.state.as_deref().unwrap_or("unknown"),
                linked_pull_request_source_label(pr)
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

fn linked_pull_request_source_label(pr: &shea_symphony::model::LinkedPullRequest) -> &'static str {
    match pr.source {
        shea_symphony::model::LinkedPullRequestSource::GithubNative => "github_native",
        shea_symphony::model::LinkedPullRequestSource::FallbackDiagnostic => "fallback_diagnostic",
        shea_symphony::model::LinkedPullRequestSource::Unknown => "unknown",
    }
}

pub(crate) fn project_relationship_list(
    workflow_path: PathBuf,
    issue_ref: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let readback = adapter.relationship_readback(&issue_ref)?;
    print_relationship_readback("relationship_list=ok", &readback);
    Ok(())
}

pub(crate) fn project_relationship_verify(
    workflow_path: PathBuf,
    issue_ref: String,
    blocked_by: Vec<String>,
    subissue: Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let readback = adapter.relationship_readback(&issue_ref)?;
    let missing_blocked_by = blocked_by
        .iter()
        .filter(|expected| !readback.has_blocker(expected))
        .cloned()
        .collect::<Vec<_>>();
    let missing_subissues = subissue
        .iter()
        .filter(|expected| !readback.has_native_subissue(expected))
        .cloned()
        .collect::<Vec<_>>();

    print_relationship_readback("relationship_verify=ok", &readback);
    if !missing_blocked_by.is_empty() || !missing_subissues.is_empty() {
        return Err(format!(
            "relationship verify failed: issue={} missing_blocked_by={} missing_subissues={}",
            readback.issue_identifier,
            missing_or_none(&missing_blocked_by),
            missing_or_none(&missing_subissues)
        )
        .into());
    }
    Ok(())
}

pub(crate) fn project_relationship_add_blocked_by(
    workflow_path: PathBuf,
    issue_ref: String,
    blocker_ref: String,
    write: bool,
    dry_run: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    project_relationship_add_blocked_by_with_adapter(
        &config,
        adapter.as_ref(),
        &issue_ref,
        &blocker_ref,
        write,
        dry_run,
    )
}

fn project_relationship_add_blocked_by_with_adapter(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    blocker_ref: &str,
    write: bool,
    dry_run: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if write && dry_run {
        return Err(
            "relationship add command accepts either --write or --dry-run, not both".into(),
        );
    }
    if dry_run {
        let readback = adapter.relationship_readback(issue_ref)?;
        println!("relationship_add_blocked_by_dry_run=ok");
        println!(
            "action=add_blocked_by issue={} blocker={} already_present={} would_add={}",
            issue_ref,
            blocker_ref,
            readback.has_blocker(blocker_ref),
            !readback.has_blocker(blocker_ref)
        );
        print_relationship_readback("relationship_dry_run_readback=ok", &readback);
        return Ok(());
    }

    require_write_intent(write)?;
    let readback = adapter.add_blocked_by_relationship(issue_ref, blocker_ref)?;
    append_tracker_mutation_audit(
        config,
        TrackerMutationAudit {
            command: "project relationship add-blocked-by",
            mutation_type: "relationship",
            issue_ref: Some(issue_ref),
            target: Some(blocker_ref.to_string()),
            from_state: None,
            to_state: None,
            reason: "native blocked-by relationship add with readback verification",
        },
    );
    print_relationship_readback("relationship_add_blocked_by=ok", &readback);
    Ok(())
}

pub(crate) fn project_relationship_add_subissue(
    workflow_path: PathBuf,
    parent_ref: String,
    subissue_ref: String,
    write: bool,
    dry_run: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    project_relationship_add_subissue_with_adapter(
        &config,
        adapter.as_ref(),
        &parent_ref,
        &subissue_ref,
        write,
        dry_run,
    )
}

fn project_relationship_add_subissue_with_adapter(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    parent_ref: &str,
    subissue_ref: &str,
    write: bool,
    dry_run: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if write && dry_run {
        return Err(
            "relationship add command accepts either --write or --dry-run, not both".into(),
        );
    }
    if dry_run {
        let readback = adapter.relationship_readback(parent_ref)?;
        println!("relationship_add_subissue_dry_run=ok");
        println!(
            "action=add_subissue parent={} subissue={} already_present={} would_add={}",
            parent_ref,
            subissue_ref,
            readback.has_native_subissue(subissue_ref),
            !readback.has_native_subissue(subissue_ref)
        );
        print_relationship_readback("relationship_dry_run_readback=ok", &readback);
        return Ok(());
    }

    require_write_intent(write)?;
    let readback = adapter.add_subissue_relationship(parent_ref, subissue_ref)?;
    append_tracker_mutation_audit(
        config,
        TrackerMutationAudit {
            command: "project relationship add-subissue",
            mutation_type: "relationship",
            issue_ref: Some(parent_ref),
            target: Some(subissue_ref.to_string()),
            from_state: None,
            to_state: None,
            reason: "native parent/subissue relationship add with readback verification",
        },
    );
    print_relationship_readback("relationship_add_subissue=ok", &readback);
    Ok(())
}

fn print_relationship_readback(prefix: &str, readback: &IssueRelationshipReadback) {
    println!("{prefix}");
    println!("issue={}", readback.issue_identifier);
    println!(
        "native_parent={}",
        readback.native_parent.as_deref().unwrap_or("")
    );
    println!("blocked_by_count={}", readback.blocked_by.len());
    for blocker in &readback.blocked_by {
        println!(
            "blocked_by={} state={} id={}",
            blocker
                .identifier
                .as_deref()
                .or(blocker.id.as_deref())
                .unwrap_or("unknown"),
            blocker.state.as_deref().unwrap_or("unknown"),
            blocker.id.as_deref().unwrap_or("")
        );
    }
    println!("native_subissue_count={}", readback.native_subissues.len());
    for subissue in &readback.native_subissues {
        println!(
            "native_subissue={} project_state={} github_state={} title={}",
            subissue.identifier,
            subissue.project_state.as_deref().unwrap_or("missing"),
            subissue.github_state.as_deref().unwrap_or("unknown"),
            subissue.title.as_deref().unwrap_or("")
        );
    }
}

fn missing_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "none".into()
    } else {
        values.join(",")
    }
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

pub(crate) fn render_project_state_json(
    issues: &[TrackerIssue],
    integration_gaps: &[String],
    scope: &str,
    terminal_states: &BTreeSet<String>,
) -> Result<String, serde_json::Error> {
    let mut state_counts = BTreeMap::new();
    let mut lane_counts = BTreeMap::from([
        ("main".to_string(), 0usize),
        ("review".to_string(), 0usize),
        ("merge".to_string(), 0usize),
    ]);
    let mut operator_issues = Vec::new();
    let mut rendered_issues = Vec::new();

    for issue in issues {
        let state = issue.state.trim();
        let state = if state.is_empty() { "(unknown)" } else { state };
        *state_counts.entry(state.to_string()).or_insert(0usize) += 1;
        if let Some(lane) = lane_for_issue(issue, terminal_states) {
            *lane_counts.entry(lane.to_string()).or_insert(0usize) += 1;
        }

        let rendered = render_queue_issue(issue);
        if is_operator_state(state) {
            operator_issues.push(rendered.clone());
        }
        rendered_issues.push(rendered);
    }

    serde_json::to_string_pretty(&json!({
        "projectStateAccess": "ok",
        "trusted": true,
        "totalOpen": issues.len(),
        "emptyQueue": issues.is_empty(),
        "stateCounts": state_counts,
        "laneCounts": lane_counts,
        "operatorIssues": operator_issues,
        "issues": rendered_issues,
        "integrationGaps": integration_gaps,
        "source": "project state",
        "scope": scope,
    }))
}

fn render_queue_issue(issue: &TrackerIssue) -> Value {
    let blocked_by = issue
        .blocked_by
        .iter()
        .map(|blocker| {
            json!({
                "id": blocker.id,
                "identifier": blocker.identifier,
                "state": blocker.state,
            })
        })
        .collect::<Vec<_>>();
    let blocked_reason = if blocked_by.is_empty() {
        None
    } else {
        Some("issue has tracker dependencies")
    };
    let native_subissues = native_subissue_statuses(issue)
        .into_iter()
        .map(|subissue| {
            json!({
                "identifier": subissue.identifier,
                "projectState": subissue.project_state,
                "githubState": subissue.github_state,
            })
        })
        .collect::<Vec<_>>();

    json!({
        "identifier": issue.identifier,
        "title": issue.title,
        "state": issue.state,
        "url": issue.url,
        "updatedAt": issue.updated_at,
        "createdAt": issue.created_at,
        "assignees": issue.assignees,
        "labels": issue.labels,
        "priority": issue.priority,
        "branchName": issue.branch_name,
        "blockedBy": blocked_by,
        "blockedReason": blocked_reason,
        "nativeSubissues": native_subissues,
    })
}

fn lane_for_issue(
    issue: &TrackerIssue,
    terminal_states: &BTreeSet<String>,
) -> Option<&'static str> {
    let normalized_state = issue.normalized_state();
    if matches!(normalized_state.as_str(), "todo" | "rework")
        && issue_has_unresolved_blockers(issue, terminal_states)
    {
        return None;
    }
    if matches!(normalized_state.as_str(), "todo" | "rework")
        && native_subissue_gate_blocker(issue, terminal_states).is_some()
    {
        return None;
    }

    match normalized_state.as_str() {
        "todo" | "rework" | "in progress" => Some("main"),
        "agent review" => Some("review"),
        "merging" => Some("merge"),
        _ => None,
    }
}

fn issue_has_unresolved_blockers(issue: &TrackerIssue, terminal_states: &BTreeSet<String>) -> bool {
    issue.blocked_by.iter().any(|blocker| {
        blocker
            .state
            .as_deref()
            .map(normalize_state)
            .map(|state| !terminal_states.contains(&state))
            .unwrap_or(true)
    })
}

fn is_operator_state(state: &str) -> bool {
    matches!(
        normalize_state(state).as_str(),
        "need to clarify" | "need human input" | "human review"
    )
}
