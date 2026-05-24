use shea_symphony::config::RuntimeConfig;
use shea_symphony::issue_forge::{next_clarification_question, ForgeValidationReport};
use shea_symphony::model::{normalize_state, GateDecision, GateDecisionKind, TrackerIssue};
use shea_symphony::tracker::adapter_from_config;
use std::path::PathBuf;

use crate::cli::ForgeStatusArg;
mod create;
mod rework;

#[cfg(test)]
pub(crate) use create::{
    find_duplicate_issue_title, forge_create_requires_assignee, render_forge_create_success,
    validate_forge_create_contract, verify_forge_created_issue_status, write_forge_created_issue,
    ForgeCreateResult, ForgeCreateWriteInput,
};
pub(crate) use create::{
    forge_create, validate_forge_create_report_with_assignees, ForgeCreateOptions,
};

pub(crate) use rework::{forge_rework, ForgeReworkOptions};
#[cfg(test)]
pub(crate) use rework::{forge_rework_with_adapter, ForgeReworkInput};

use crate::orchestration::{append_tracker_mutation_audit, load_config, TrackerMutationAudit};

pub(crate) fn forge_promote(
    workflow_path: PathBuf,
    issue_ref: String,
    title: String,
    markdown: String,
    promotion_note: PromotionNoteInput,
    write: bool,
    dry_run: bool,
) -> Result<(), Box<dyn std::error::Error>> {
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

    let report = forge_validation_report(
        ForgeStatusArg::Todo,
        &title,
        &markdown,
        &config,
        &source.assignees,
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

    let write_readbacks = vec![format!(
        "`forge promote --write` updated the existing issue content; pre-status readback confirmed issue `{}` title `{}` before the final Project status mutation.",
        content_verified.identifier, content_verified.title
    )];
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
    match status {
        ForgeStatusArg::Backlog => Ok(validate_backlog_seed(title, markdown)),
        ForgeStatusArg::Todo => {
            validate_forge_create_report_with_assignees(title, markdown, config, intended_assignees)
        }
    }
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
