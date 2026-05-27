use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use shea_symphony::config::RuntimeConfig;
use shea_symphony::issue_forge::next_clarification_question;
use shea_symphony::model::{normalize_state, TrackerIssue};
use shea_symphony::tracker::{
    adapter_from_config, FollowUpIssueInput, ProjectFieldAssignment, TrackerAdapter,
};

use crate::cli::ForgeStatusArg;
use crate::commands::gate::evaluate_issue_for_current_source;
use crate::orchestration::{append_tracker_mutation_audit, load_config, TrackerMutationAudit};

use super::{
    apply_forge_relationship_plan, blocker_refs_from_relationship_plan,
    forge_validation_report_with_relationships, print_forge_validation, ForgeRelationshipPlan,
};

#[derive(Debug, Clone)]
pub(crate) struct ForgeCreateOptions {
    pub(crate) workflow_path: PathBuf,
    pub(crate) title: String,
    pub(crate) markdown: String,
    pub(crate) status: ForgeStatusArg,
    pub(crate) project: Option<String>,
    pub(crate) project_fields: Vec<ProjectFieldAssignment>,
    pub(crate) assignees: Vec<String>,
    pub(crate) relationships: ForgeRelationshipPlan,
    pub(crate) write: bool,
    pub(crate) dry_run: bool,
}

pub(crate) fn forge_create(options: ForgeCreateOptions) -> Result<(), Box<dyn std::error::Error>> {
    let ForgeCreateOptions {
        workflow_path,
        title,
        markdown,
        status,
        project,
        project_fields,
        assignees,
        relationships,
        write,
        dry_run,
    } = options;
    if write && dry_run {
        return Err("forge create cannot use --write and --dry-run together".into());
    }
    let dry_run = !write || dry_run;
    let config = load_config(&workflow_path)?;
    let project_label = validate_forge_project_selection(&config, project.as_deref())?;
    let assignees = normalize_forge_assignees(assignees);
    if forge_create_requires_assignee(&config, status) && assignees.is_empty() {
        return Err(
            "forge create --status Todo requires --assignee for live GitHub issue creation".into(),
        );
    }
    let report = forge_validation_report_with_relationships(
        status,
        &title,
        &markdown,
        &config,
        &assignees,
        &relationships,
    )?;
    print_forge_validation(&report);
    if !report.decision.is_dispatchable() {
        return Err(format!(
            "forge create validation failed for status {}; tracker issue was not created",
            status.as_str()
        )
        .into());
    }

    if dry_run {
        println!(
            "forge_create_dry_run=ok status={} project={} title={:?} project_fields={}",
            status.as_str(),
            project_label,
            report.title,
            project_fields.len()
        );
        if !relationships.is_empty() {
            println!(
                "relationship_plan=planned blocked_by={} parent={}",
                relationships.blocked_by.join(","),
                relationships.parent.as_deref().unwrap_or("")
            );
        }
        return Ok(());
    }

    let adapter = adapter_from_config(&config);
    let create_result = write_forge_created_issue(
        &config,
        adapter.as_ref(),
        ForgeCreateWriteInput {
            title: report.title,
            markdown,
            assignees,
            status,
            project_label: &project_label,
            project_fields: &project_fields,
            relationships: &relationships,
        },
    )?;

    println!(
        "{}",
        render_forge_create_success(&create_result, status, project_fields.len())
    );
    Ok(())
}

pub(crate) struct ForgeCreateWriteInput<'a> {
    pub(crate) title: String,
    pub(crate) markdown: String,
    pub(crate) assignees: Vec<String>,
    pub(crate) status: ForgeStatusArg,
    pub(crate) project_label: &'a str,
    pub(crate) project_fields: &'a [ProjectFieldAssignment],
    pub(crate) relationships: &'a ForgeRelationshipPlan,
}

#[derive(Debug, Clone)]
pub(crate) struct ForgeCreateResult {
    pub(crate) issue_id: String,
    pub(crate) readback: Option<TrackerIssue>,
}

pub(crate) fn write_forge_created_issue(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    input: ForgeCreateWriteInput<'_>,
) -> Result<ForgeCreateResult, Box<dyn std::error::Error>> {
    let existing_issues = adapter.list_project_summary_issues()?;
    if let Some(duplicate) = find_duplicate_issue_title(&existing_issues, &input.title) {
        return Err(format!(
            "duplicate tracker issue title detected: {} {}",
            duplicate.identifier,
            duplicate.url.as_deref().unwrap_or(&duplicate.title)
        )
        .into());
    }

    let issue_id = adapter.create_follow_up_issue(FollowUpIssueInput {
        title: input.title,
        body: input.markdown,
        assignees: input.assignees,
        project_id: None,
        related_issue_ref: None,
        blocked_by_issue_ref: None,
    })?;
    append_tracker_mutation_audit(
        config,
        TrackerMutationAudit {
            command: "forge create",
            mutation_type: "issue_create",
            issue_ref: None,
            target: Some(issue_id.clone()),
            from_state: None,
            to_state: None,
            reason: "quality-gated forge issue creation",
        },
    );

    let stage_state = if input.status == ForgeStatusArg::Todo && !input.relationships.is_empty() {
        "backlog"
    } else {
        input.status.normalized_state()
    };
    adapter.add_issue_to_project_with_state(&issue_id, stage_state)?;
    append_tracker_mutation_audit(
        config,
        TrackerMutationAudit {
            command: "forge create",
            mutation_type: "project_add",
            issue_ref: Some(&issue_id),
            target: Some(input.project_label.into()),
            from_state: None,
            to_state: Some(stage_state.into()),
            reason: "forge issue added to project with requested initial status",
        },
    );
    for assignment in input.project_fields {
        adapter.set_project_field(&issue_id, assignment)?;
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "forge create",
                mutation_type: "project_field",
                issue_ref: Some(&issue_id),
                target: Some(format!("{}={}", assignment.name, assignment.value)),
                from_state: None,
                to_state: None,
                reason: "forge project field assignment",
            },
        );
    }
    if !input.relationships.is_empty() {
        apply_forge_relationship_plan(adapter, &issue_id, input.relationships)?;
    }
    if input.status == ForgeStatusArg::Todo && stage_state != "todo" {
        adapter.set_state(&issue_id, "todo")?;
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "forge create",
                mutation_type: "status",
                issue_ref: Some(&issue_id),
                target: Some("Todo".into()),
                from_state: Some("backlog".into()),
                to_state: Some("todo".into()),
                reason: "forge relationship-verified Todo creation final status update",
            },
        );
    }

    let readback = verify_forge_created_issue_status(adapter, &issue_id, input.status)?;
    Ok(ForgeCreateResult { issue_id, readback })
}

pub(crate) fn verify_forge_created_issue_status(
    adapter: &dyn TrackerAdapter,
    issue_id: &str,
    status: ForgeStatusArg,
) -> Result<Option<TrackerIssue>, Box<dyn std::error::Error>> {
    let expected = normalize_state(status.normalized_state());
    let mut last_state = None;
    let mut last_issue = None;

    for attempt in 0..3 {
        if let Some(issue) = adapter.get_issue(issue_id)? {
            let actual = issue.normalized_state();
            if actual == expected {
                return Ok(Some(issue));
            }
            last_state = Some(issue.state.clone());
            last_issue = Some(issue);
        } else if adapter.kind() == "memory" {
            return Ok(None);
        }

        if attempt < 2 {
            thread::sleep(Duration::from_millis(500));
        }
    }

    if let Some(actual) = last_state {
        Err(format!(
            "forge create stopped at readback: expected Project status {}, got {:?} for {}",
            status.as_str(),
            actual,
            render_forge_created_issue_location(issue_id, last_issue.as_ref())
        )
        .into())
    } else {
        Err(format!(
            "forge create stopped at readback: {} was not found in the configured Project after creation",
            render_forge_created_issue_location(issue_id, None)
        )
        .into())
    }
}

pub(crate) fn render_forge_create_success(
    result: &ForgeCreateResult,
    status: ForgeStatusArg,
    project_fields_count: usize,
) -> String {
    let mut fields = vec![
        "forge_create=ok".to_string(),
        format!("issue_id={}", result.issue_id),
    ];
    if let Some(readback) = result.readback.as_ref() {
        append_forge_created_issue_readback_fields(&mut fields, &result.issue_id, readback);
    }
    fields.push(format!("status={}", status.as_str()));
    if let Some(readback) = result.readback.as_ref() {
        fields.push(format!(
            "project_status={}",
            forge_created_issue_project_status(readback)
        ));
    }
    fields.push(format!("project_fields={project_fields_count}"));
    fields.join(" ")
}

fn render_forge_created_issue_location(issue_id: &str, readback: Option<&TrackerIssue>) -> String {
    let mut fields = vec![format!("issue_id={issue_id}")];
    if let Some(readback) = readback {
        append_forge_created_issue_readback_fields(&mut fields, issue_id, readback);
    }
    fields.join(" ")
}

fn append_forge_created_issue_readback_fields(
    fields: &mut Vec<String>,
    issue_id: &str,
    readback: &TrackerIssue,
) {
    if !readback.identifier.is_empty() && readback.identifier != issue_id {
        fields.push(format!("issue={}", readback.identifier));
    }
    if let Some(url) = readback.url.as_deref().filter(|url| !url.is_empty()) {
        fields.push(format!("url={url}"));
    }
}

fn forge_created_issue_project_status(readback: &TrackerIssue) -> String {
    readback
        .project_fields
        .get("Status")
        .and_then(|status| status.as_str())
        .filter(|status| !status.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| readback.state.clone())
}

fn validate_forge_project_selection(
    config: &RuntimeConfig,
    project: Option<&str>,
) -> Result<String, Box<dyn std::error::Error>> {
    let owner = config
        .tracker
        .project_owner
        .as_deref()
        .unwrap_or("workflow");
    let number = config
        .tracker
        .project_number
        .map(|number| number.to_string())
        .unwrap_or_else(|| "configured".into());
    let configured = format!("{owner}/{number}");
    let Some(project) = project.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(configured);
    };
    if matches!(project, "default" | "workflow") || project == number || project == configured {
        Ok(configured)
    } else {
        Err(format!(
            "forge create --project currently supports the configured workflow Project only ({configured}); got {project:?}"
        )
        .into())
    }
}

fn normalize_forge_assignees(assignees: Vec<String>) -> Vec<String> {
    assignees
        .into_iter()
        .map(|assignee| assignee.trim().trim_start_matches('@').to_string())
        .filter(|assignee| !assignee.is_empty())
        .collect()
}

pub(crate) fn forge_create_requires_assignee(
    config: &RuntimeConfig,
    status: ForgeStatusArg,
) -> bool {
    status == ForgeStatusArg::Todo
        && config.tracker.kind == "github_project_v2"
        && config.tracker.fixture_path.is_none()
        && !config.tracker.assignee_filter.allow_unassigned
}

pub(crate) fn find_duplicate_issue_title<'a>(
    issues: &'a [TrackerIssue],
    title: &str,
) -> Option<&'a TrackerIssue> {
    let title_key = normalized_issue_title_key(title);
    issues
        .iter()
        .find(|issue| normalized_issue_title_key(&issue.title) == title_key)
}

fn normalized_issue_title_key(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

#[cfg(test)]
pub(crate) fn validate_forge_create_contract(
    title: &str,
    markdown: &str,
    config: &RuntimeConfig,
    intended_assignees: &[String],
) -> Result<shea_symphony::issue_forge::ForgeValidationReport, String> {
    let report =
        validate_forge_create_report_with_assignees(title, markdown, config, intended_assignees)
            .map_err(|error| format!("source alignment failed: {error}"))?;
    if report.decision.is_dispatchable() {
        Ok(report)
    } else {
        Err("issue forge validation failed; tracker issue was not created".into())
    }
}

#[cfg(test)]
pub(crate) fn validate_forge_create_report_with_assignees(
    title: &str,
    markdown: &str,
    config: &RuntimeConfig,
    intended_assignees: &[String],
) -> Result<shea_symphony::issue_forge::ForgeValidationReport, Box<dyn std::error::Error>> {
    validate_forge_create_report_with_relationships(
        title,
        markdown,
        config,
        intended_assignees,
        &ForgeRelationshipPlan::default(),
    )
}

pub(crate) fn validate_forge_create_report_with_relationships(
    title: &str,
    markdown: &str,
    config: &RuntimeConfig,
    intended_assignees: &[String],
    relationships: &ForgeRelationshipPlan,
) -> Result<shea_symphony::issue_forge::ForgeValidationReport, Box<dyn std::error::Error>> {
    let issue = TrackerIssue {
        tracker_kind: config.tracker.kind.clone(),
        id: "forge-issue-draft".into(),
        item_id: None,
        identifier: "#draft".into(),
        title: title.into(),
        description: Some(markdown.into()),
        url: None,
        state: config.tracker.state_map.todo.clone(),
        labels: Vec::new(),
        assignees: intended_assignees.to_vec(),
        priority: None,
        branch_name: None,
        linked_pull_requests: Vec::new(),
        blocked_by: blocker_refs_from_relationship_plan(relationships, config),
        project_fields: Default::default(),
        created_at: None,
        updated_at: None,
    };
    let decision = evaluate_issue_for_current_source(config, &issue)?;
    Ok(shea_symphony::issue_forge::ForgeValidationReport {
        title: title.to_string(),
        question: next_clarification_question(&decision),
        decision,
    })
}
