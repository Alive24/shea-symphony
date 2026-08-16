use shea_symphony::config::RuntimeConfig;
use shea_symphony::issue_forge::{next_clarification_question, ForgeValidationReport};
use shea_symphony::model::{
    normalize_state, BlockerRef, GateDecision, GateDecisionKind, TrackerIssue,
};
use shea_symphony::tracker::{adapter_from_config, TrackerAdapter};
use shea_symphony::workpad_templates::{render_workpad_template, WorkpadTemplateId};
use shea_symphony::{
    handoff::parent_integration_branch_name,
    workspace::safe_identifier as workspace_safe_identifier,
};
use std::path::PathBuf;

use crate::cli::ForgeStatusArg;
mod create;
mod rework;

#[cfg(test)]
pub(crate) use create::{
    find_duplicate_issue_title, forge_create_requires_assignee, render_forge_create_success,
    validate_forge_create_contract, validate_forge_create_report_with_assignees,
    verify_forge_created_issue_status, write_forge_created_issue, ForgeCreateResult,
    ForgeCreateWriteInput,
};
pub(crate) use create::{forge_create, ForgeCreateOptions};

pub(crate) use rework::{forge_rework, ForgeReworkOptions};
#[cfg(test)]
pub(crate) use rework::{forge_rework_with_adapter, ForgeReworkInput};

use crate::orchestration::{append_tracker_mutation_audit, load_config, TrackerMutationAudit};

pub(crate) struct ForgePromoteInput {
    pub(crate) workflow_path: PathBuf,
    pub(crate) issue_ref: String,
    pub(crate) title: String,
    pub(crate) markdown: String,
    pub(crate) promotion_note: PromotionNoteInput,
    pub(crate) relationships: ForgeRelationshipPlan,
    pub(crate) write: bool,
    pub(crate) dry_run: bool,
}

pub(crate) fn forge_promote(input: ForgePromoteInput) -> Result<(), Box<dyn std::error::Error>> {
    let ForgePromoteInput {
        workflow_path,
        issue_ref,
        title,
        markdown,
        promotion_note,
        relationships,
        write,
        dry_run,
    } = input;

    if write && dry_run {
        return Err("forge promote cannot use --write and --dry-run together".into());
    }
    let dry_run = !write || dry_run;
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let source = adapter
        .get_issue(&issue_ref)
        .map_err(|error| format!("forge promote stopped at read_source: {error}"))?
        .ok_or_else(|| {
            format!("forge promote stopped at read_source: issue not found: {issue_ref}")
        })?;
    if normalize_state(&source.state) != normalize_state(&config.tracker.state_map.backlog) {
        return Err(format!(
            "forge promote stopped at preflight: {} is in {:?}, expected Backlog",
            source.identifier, source.state
        )
        .into());
    }

    let report = forge_validation_report_with_relationships(
        ForgeStatusArg::Todo,
        &title,
        &markdown,
        &config,
        &source.assignees,
        &relationships,
    )
    .map_err(|error| format!("forge promote stopped at validate: {error}"))?;
    print_forge_validation(&report);
    if !report.decision.is_dispatchable() {
        return Err("forge promote stopped at validate: promoted body failed Todo gate".into());
    }

    if dry_run {
        let dry_run_readbacks = vec![
            "`forge promote --dry-run` validated the promoted body and promotion note inputs."
                .to_string(),
        ];
        let note = render_promotion_note(
            &source.identifier,
            &report.title,
            &promotion_note,
            &dry_run_readbacks,
        );
        println!(
            "forge_promote_dry_run=ok issue={} from=Backlog to=Todo title={:?}",
            source.identifier, report.title
        );
        if !relationships.is_empty() {
            println!(
                "relationship_plan=planned blocked_by={} parent={}",
                relationships.blocked_by.join(","),
                relationships.parent.as_deref().unwrap_or("")
            );
        }
        println!("promotion_note_preview=\n{note}");
        return Ok(());
    }

    adapter
        .update_issue_content(&source.identifier, &report.title, &markdown)
        .map_err(|error| format!("forge promote stopped at edit_issue: {error}"))?;
    append_tracker_mutation_audit(
        &config,
        TrackerMutationAudit {
            command: "forge promote",
            mutation_type: "issue_edit",
            issue_ref: Some(&source.identifier),
            target: Some(report.title.clone()),
            from_state: Some(source.state.clone()),
            to_state: None,
            reason: "forge backlog promotion content update",
        },
    );

    let content_verified = adapter
        .get_issue(&source.identifier)
        .map_err(|error| format!("forge promote stopped at readback: {error}"))?
        .ok_or_else(|| {
            format!(
                "forge promote stopped at readback: issue disappeared after update: {}",
                source.identifier
            )
        })?;
    if content_verified.title != report.title {
        return Err(format!(
            "forge promote stopped at readback: expected title {:?}, got title {:?}",
            report.title, content_verified.title
        )
        .into());
    }

    let relationship_readbacks = apply_forge_relationship_plan(
        &config,
        adapter.as_ref(),
        &content_verified.identifier,
        &relationships,
    )
    .map_err(|error| format!("forge promote stopped at relationships: {error}"))?;

    let mut write_readbacks = vec![format!(
        "`forge promote --write` updated the existing issue content; pre-status readback confirmed issue `{}` title `{}` before the final Project status mutation.",
        content_verified.identifier, content_verified.title
    )];
    write_readbacks.extend(relationship_readbacks);
    let note = render_promotion_note(
        &source.identifier,
        &content_verified.title,
        &promotion_note,
        &write_readbacks,
    );
    adapter
        .add_issue_comment(&content_verified.identifier, &note)
        .map_err(|error| format!("forge promote stopped at promotion_note: {error}"))?;
    append_tracker_mutation_audit(
        &config,
        TrackerMutationAudit {
            command: "forge promote",
            mutation_type: "comment",
            issue_ref: Some(&content_verified.identifier),
            target: Some("Promotion Note".into()),
            from_state: Some(source.state.clone()),
            to_state: None,
            reason: "forge backlog promotion note",
        },
    );

    adapter
        .set_state(&source.identifier, "todo")
        .map_err(|error| format!("forge promote stopped at set_status: {error}"))?;
    append_tracker_mutation_audit(
        &config,
        TrackerMutationAudit {
            command: "forge promote",
            mutation_type: "status",
            issue_ref: Some(&source.identifier),
            target: Some("Todo".into()),
            from_state: Some(source.state.clone()),
            to_state: Some("todo".into()),
            reason: "forge backlog promotion final status update",
        },
    );

    let verified = adapter
        .get_issue(&source.identifier)
        .map_err(|error| format!("forge promote stopped at final_readback: {error}"))?
        .ok_or_else(|| {
            format!(
                "forge promote stopped at final_readback: issue disappeared after update: {}",
                source.identifier
            )
        })?;
    let status_ok =
        normalize_state(&verified.state) == normalize_state(&config.tracker.state_map.todo);
    let title_ok = verified.title == report.title;
    if !status_ok || !title_ok {
        return Err(format!(
            "forge promote stopped at final_readback: expected title {:?} and Todo, got title {:?} and state {:?}",
            report.title, verified.title, verified.state
        )
        .into());
    }

    println!(
        "forge_promote=ok issue={} status=Todo title={:?} promotion_note=commented final_status_mutation=true",
        verified.identifier, verified.title
    );
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PromotionNoteInput {
    pub(crate) operator_confirmation: String,
    pub(crate) decisions: Vec<String>,
    pub(crate) scope_changes: Vec<String>,
    pub(crate) dependencies_context: Vec<String>,
    pub(crate) readback_summaries: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ForgeRelationshipPlan {
    pub(crate) blocked_by: Vec<String>,
    pub(crate) parent: Option<String>,
}

impl ForgeRelationshipPlan {
    pub(crate) fn is_empty(&self) -> bool {
        self.blocked_by.is_empty() && self.parent.is_none()
    }
}

pub(crate) fn apply_forge_relationship_plan(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    relationships: &ForgeRelationshipPlan,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut readbacks = Vec::new();
    for blocker_ref in &relationships.blocked_by {
        let readback = adapter.add_blocked_by_relationship(issue_ref, blocker_ref)?;
        readbacks.push(format!(
            "`{}` blocked-by `{}` readback verified (blocked_by_count={}).",
            readback.issue_identifier,
            blocker_ref,
            readback.blocked_by.len()
        ));
    }
    if let Some(parent_ref) = &relationships.parent {
        let readback = adapter.add_subissue_relationship(parent_ref, issue_ref)?;
        readbacks.push(format!(
            "`{}` native subissue `{}` readback verified (native_subissue_count={}).",
            parent_ref,
            issue_ref,
            readback.native_subissues.len()
        ));
        if let Some(parent_readback) =
            ensure_forge_parent_integration_branch_evidence(config, adapter, parent_ref, issue_ref)?
        {
            readbacks.push(parent_readback);
        }
    }
    Ok(readbacks)
}

fn ensure_forge_parent_integration_branch_evidence(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    parent_ref: &str,
    first_subissue_ref: &str,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let Some(parent_issue) = adapter.get_issue(parent_ref)? else {
        return Ok(None);
    };
    let parent_integration_branch =
        parent_integration_branch_name(&parent_issue.identifier, Some(&parent_issue.title));
    if parent_issue_has_integration_branch_evidence(&parent_issue, &parent_integration_branch) {
        return Ok(Some(format!(
            "`{}` parent integration branch `{}` already recorded.",
            parent_issue.identifier, parent_integration_branch
        )));
    }

    let workpad = render_forge_parent_topology_workpad(
        &parent_issue,
        first_subissue_ref,
        &parent_integration_branch,
        config.git_base_branch(),
    );
    adapter.upsert_workpad(&parent_issue.identifier, &workpad)?;
    append_tracker_mutation_audit(
        config,
        TrackerMutationAudit {
            command: "forge relationship",
            mutation_type: "workpad_write",
            issue_ref: Some(&parent_issue.identifier),
            target: Some(first_subissue_ref.to_string()),
            from_state: Some(parent_issue.state.clone()),
            to_state: None,
            reason: "parent integration branch evidence after native subissue relationship",
        },
    );
    Ok(Some(format!(
        "`{}` parent integration branch `{}` recorded before subissue dispatch.",
        parent_issue.identifier, parent_integration_branch
    )))
}

fn parent_issue_has_integration_branch_evidence(issue: &TrackerIssue, branch: &str) -> bool {
    issue.branch_name.as_deref() == Some(branch)
        || issue
            .description
            .as_deref()
            .is_some_and(|description| description.contains(branch))
        || issue
            .project_fields
            .values()
            .any(|value| project_field_contains_branch(value, branch))
}

fn project_field_contains_branch(value: &serde_json::Value, branch: &str) -> bool {
    match value {
        serde_json::Value::String(value) => value == branch || value.contains(branch),
        serde_json::Value::Array(values) => values
            .iter()
            .any(|value| project_field_contains_branch(value, branch)),
        serde_json::Value::Object(values) => values
            .values()
            .any(|value| project_field_contains_branch(value, branch)),
        _ => false,
    }
}

fn render_forge_parent_topology_workpad(
    parent_issue: &TrackerIssue,
    first_subissue_ref: &str,
    parent_integration_branch: &str,
    parent_final_base_branch: &str,
) -> String {
    let workspace_key = workspace_safe_identifier(&format!(
        "issue-{}",
        parent_issue.identifier.trim_start_matches('#')
    ));
    render_workpad_template(
        None,
        WorkpadTemplateId::ParentTopology,
        &[
            ("parent_issue_ref", parent_issue.identifier.clone()),
            ("parent_issue_title", parent_issue.title.clone()),
            ("issue_ref", first_subissue_ref.into()),
            ("issue_title", "first native subissue".into()),
            ("parent_integration_branch", parent_integration_branch.into()),
            ("parent_final_base_branch", parent_final_base_branch.into()),
            (
                "source",
                "`shea-symphony forge relationship parent topology ensure`".into(),
            ),
            (
                "runtime_identity",
                format!(
                    "- Issue: `{}`\n- Workspace key: `{workspace_key}`\n- Branch: `{parent_integration_branch}`",
                    parent_issue.identifier
                ),
            ),
        ],
    )
    .expect("repository Markdown parent topology template must render")
}

pub(crate) fn render_promotion_note(
    source_issue: &str,
    promoted_title: &str,
    input: &PromotionNoteInput,
    generated_readbacks: &[String],
) -> String {
    let mut lines = vec![
        "## Promotion Note".to_string(),
        String::new(),
        format!("- Source Backlog issue: {source_issue}"),
        format!("- Promoted Todo title/status: `{promoted_title}` / `Todo`"),
        format!(
            "- Operator confirmation: {:?}",
            input.operator_confirmation.trim()
        ),
        String::new(),
        "## Key Operator Decisions".to_string(),
        String::new(),
    ];
    push_markdown_bullets(&mut lines, &input.decisions);
    lines.extend([
        String::new(),
        "## Major Scope Changes From Seed".to_string(),
        String::new(),
    ]);
    push_markdown_bullets(&mut lines, &input.scope_changes);
    lines.extend([
        String::new(),
        "## Dependencies and Context".to_string(),
        String::new(),
    ]);
    push_markdown_bullets(&mut lines, &input.dependencies_context);
    lines.extend([
        String::new(),
        "## Verification Readback".to_string(),
        String::new(),
    ]);
    push_markdown_bullets(&mut lines, generated_readbacks);
    push_markdown_bullets(&mut lines, &input.readback_summaries);
    lines.join("\n")
}

fn push_markdown_bullets(lines: &mut Vec<String>, values: &[String]) {
    for value in values {
        lines.push(format!("- {}", value.trim()));
    }
}

pub(crate) fn forge_validate(
    workflow_path: PathBuf,
    status: Option<ForgeStatusArg>,
    title: String,
    markdown: String,
    issue_ref: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let (status, title, markdown, assignees) = if let Some(issue_ref) = issue_ref {
        let adapter = adapter_from_config(&config);
        let issue = adapter
            .get_issue(&issue_ref)?
            .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
        let status = status.unwrap_or_else(|| forge_status_from_issue(&config, &issue));
        let title = if title.trim().is_empty() {
            issue.title.clone()
        } else {
            title
        };
        let markdown = if markdown.trim().is_empty() {
            issue.description.clone().unwrap_or_default()
        } else {
            markdown
        };
        (status, title, markdown, issue.assignees)
    } else {
        let assignees = issue_contract_assignees(&markdown);
        (
            status.unwrap_or(ForgeStatusArg::Todo),
            title,
            markdown,
            assignees,
        )
    };
    let report = forge_validation_report(status, &title, &markdown, &config, &assignees)?;
    print_forge_validation(&report);
    println!("status={}", status.as_str());
    Ok(())
}

pub(crate) fn issue_contract_assignees(markdown: &str) -> Vec<String> {
    markdown
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim().trim_start_matches('-').trim();
            trimmed
                .strip_prefix("Assignee:")
                .or_else(|| trimmed.strip_prefix("Assignees:"))
        })
        .flat_map(|value| value.split(','))
        .map(|assignee| assignee.trim().trim_start_matches('@').to_string())
        .filter(|assignee| !assignee.is_empty() && !assignee.eq_ignore_ascii_case("none"))
        .collect()
}

pub(crate) fn forge_validation_report(
    status: ForgeStatusArg,
    title: &str,
    markdown: &str,
    config: &RuntimeConfig,
    intended_assignees: &[String],
) -> Result<ForgeValidationReport, Box<dyn std::error::Error>> {
    forge_validation_report_with_relationships(
        status,
        title,
        markdown,
        config,
        intended_assignees,
        &ForgeRelationshipPlan::default(),
    )
}

pub(crate) fn forge_validation_report_with_relationships(
    status: ForgeStatusArg,
    title: &str,
    markdown: &str,
    config: &RuntimeConfig,
    intended_assignees: &[String],
    relationships: &ForgeRelationshipPlan,
) -> Result<ForgeValidationReport, Box<dyn std::error::Error>> {
    match status {
        ForgeStatusArg::Backlog => Ok(validate_backlog_seed(title, markdown)),
        ForgeStatusArg::Todo => create::validate_forge_create_report_with_relationships(
            title,
            markdown,
            config,
            intended_assignees,
            relationships,
        ),
    }
}

pub(crate) fn blocker_refs_from_relationship_plan(
    relationships: &ForgeRelationshipPlan,
    config: &RuntimeConfig,
) -> Vec<BlockerRef> {
    relationships
        .blocked_by
        .iter()
        .map(|blocker_ref| BlockerRef {
            id: None,
            identifier: Some(blocker_ref.clone()),
            state: Some(config.tracker.state_map.done.clone()),
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForgeMissingCategories {
    pub(crate) candidate_missing: Vec<String>,
    pub(crate) live_context_missing: Vec<String>,
}

pub(crate) fn forge_missing_categories(report: &ForgeValidationReport) -> ForgeMissingCategories {
    let (live_context_missing, candidate_missing): (Vec<_>, Vec<_>) = report
        .decision
        .missing
        .iter()
        .cloned()
        .partition(|missing| is_live_context_missing(missing));
    ForgeMissingCategories {
        candidate_missing,
        live_context_missing,
    }
}

fn is_live_context_missing(missing: &str) -> bool {
    matches!(missing, "live GitHub issue assignee")
}

fn validate_backlog_seed(title: &str, markdown: &str) -> ForgeValidationReport {
    let mut missing = Vec::new();
    if title.trim().is_empty() {
        missing.push("title".into());
    }
    if markdown.trim().chars().count() < 40 {
        missing.push("body with enough context to revisit later".into());
    }
    if !markdown.contains("## Issue Goal") && !markdown.contains("## Issue Context") {
        missing.push("at least one Issue Goal or Issue Context section".into());
    }
    let decision = if missing.is_empty() {
        GateDecision::ready()
    } else {
        GateDecision {
            kind: GateDecisionKind::NeedToClarify,
            missing,
            assumptions: Vec::new(),
            notes: vec![
                "Backlog seed gate is intentionally lighter than the Todo Issue Quality Gate."
                    .into(),
            ],
        }
    };
    ForgeValidationReport {
        title: title.to_string(),
        question: next_clarification_question(&decision),
        decision,
    }
}

fn forge_status_from_issue(config: &RuntimeConfig, issue: &TrackerIssue) -> ForgeStatusArg {
    if normalize_state(&issue.state) == normalize_state(&config.tracker.state_map.backlog) {
        ForgeStatusArg::Backlog
    } else {
        ForgeStatusArg::Todo
    }
}

fn print_forge_validation(report: &ForgeValidationReport) {
    let categories = forge_missing_categories(report);
    println!("title={}", report.title);
    println!("gate={:?}", report.decision.kind);
    println!("dispatchable={}", report.decision.is_dispatchable());
    if !report.decision.missing.is_empty() {
        println!("missing={}", report.decision.missing.join(", "));
    }
    println!(
        "candidate_missing={}",
        missing_category_value(&categories.candidate_missing)
    );
    println!(
        "live_context_missing={}",
        missing_category_value(&categories.live_context_missing)
    );
    if !report.decision.assumptions.is_empty() {
        println!("assumptions={}", report.decision.assumptions.join("; "));
    }
    if let Some(question) = &report.question {
        println!("question={}", question.question);
        println!("why={}", question.why_it_matters);
    }
}

fn missing_category_value(values: &[String]) -> String {
    if values.is_empty() {
        "none".into()
    } else {
        values.join(", ")
    }
}
