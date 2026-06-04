use std::path::PathBuf;

use shea_symphony::config::RuntimeConfig;
use shea_symphony::lane_claim::{LaneClaim, LaneClaimLane};
use shea_symphony::model::{normalize_state, TrackerIssue};
use shea_symphony::tracker::{adapter_from_config, TrackerAdapter};
use shea_symphony::workpad_templates::{render_workpad_template, WorkpadTemplateId};

use crate::cli::ForgeStatusArg;
use crate::commands::session::timeline_pr_summary;
use crate::lanes::claim::project_text_field;
use crate::orchestration::{
    append_tracker_mutation_audit, current_gmt_timestamp, load_config,
    preflight_canonical_checkout_for_write_mode, TrackerMutationAudit,
};

use super::{forge_validation_report, print_forge_validation, push_markdown_bullets};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForgeReworkOptions {
    pub(crate) workflow_path: PathBuf,
    pub(crate) issue_ref: String,
    pub(crate) title: String,
    pub(crate) markdown: String,
    pub(crate) evidence: String,
    pub(crate) operator_confirmation: String,
    pub(crate) write: bool,
    pub(crate) dry_run: bool,
}

pub(crate) fn forge_rework(options: ForgeReworkOptions) -> Result<(), Box<dyn std::error::Error>> {
    let ForgeReworkOptions {
        workflow_path,
        issue_ref,
        title,
        markdown,
        evidence,
        operator_confirmation,
        write,
        dry_run,
    } = options;
    if write && dry_run {
        return Err("forge rework cannot use --write and --dry-run together".into());
    }
    let dry_run = !write || dry_run;
    let config = load_config(&workflow_path)?;
    preflight_canonical_checkout_for_write_mode(&config, "forge rework", write)?;
    let adapter = adapter_from_config(&config);
    forge_rework_with_adapter(
        &config,
        adapter.as_ref(),
        ForgeReworkInput {
            issue_ref,
            title,
            markdown,
            evidence,
            operator_confirmation,
            dry_run,
        },
    )
}

#[derive(Debug, Clone)]
pub(crate) struct ForgeReworkInput {
    pub(crate) issue_ref: String,
    pub(crate) title: String,
    pub(crate) markdown: String,
    pub(crate) evidence: String,
    pub(crate) operator_confirmation: String,
    pub(crate) dry_run: bool,
}

pub(crate) fn forge_rework_with_adapter(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    input: ForgeReworkInput,
) -> Result<(), Box<dyn std::error::Error>> {
    let confirmation = clean_rework_text(&input.operator_confirmation, "--operator-confirmation")?;
    let evidence = clean_rework_text(&input.evidence, "--evidence-file")?;
    let source = adapter
        .get_issue(&input.issue_ref)
        .map_err(|error| format!("forge rework stopped at read_source: {error}"))?
        .ok_or_else(|| {
            format!(
                "forge rework stopped at read_source: issue not found: {}",
                input.issue_ref
            )
        })?;
    if normalize_state(&source.state) != normalize_state(&config.tracker.state_map.human_review) {
        return Err(format!(
            "forge rework stopped at preflight: {} is in {:?}, expected Human Review",
            source.identifier, source.state
        )
        .into());
    }
    if let Err(error) = ensure_no_active_human_review_lane_claims(&source) {
        if !input.dry_run {
            let diagnostic = render_forge_rework_blocked_workpad(&source, &error.to_string());
            adapter
                .add_issue_comment(&source.identifier, &diagnostic)
                .map_err(|write_error| {
                    format!("forge rework stopped at active_claim_diagnostic: {write_error}")
                })?;
            append_tracker_mutation_audit(
                config,
                TrackerMutationAudit {
                    command: "forge rework",
                    mutation_type: "timeline_comment",
                    issue_ref: Some(&source.identifier),
                    target: Some("Rework Revision Blocker".into()),
                    from_state: Some(source.state.clone()),
                    to_state: None,
                    reason: "forge human review rework active claim diagnostic",
                },
            );
        }
        return Err(error);
    }

    let report = forge_validation_report(
        ForgeStatusArg::Todo,
        &input.title,
        &input.markdown,
        config,
        &source.assignees,
    )
    .map_err(|error| format!("forge rework stopped at validate: {error}"))?;
    print_forge_validation(&report);
    if !report.decision.is_dispatchable() {
        return Err(
            "forge rework stopped at validate: replacement body failed executable issue gate"
                .into(),
        );
    }

    if input.dry_run {
        let note = render_forge_rework_workpad(
            &source,
            &report.title,
            &confirmation,
            &evidence,
            &[
                "`forge rework --dry-run` validated Human Review source state, lane claims, replacement body, and evidence inputs.".into(),
            ],
        );
        println!(
            "forge_rework_dry_run=ok issue={} from=HumanReview to=Rework title={:?}",
            source.identifier, report.title
        );
        println!("rework_evidence_preview=\n{note}");
        return Ok(());
    }

    adapter
        .update_issue_content(&source.identifier, &report.title, &input.markdown)
        .map_err(|error| format!("forge rework stopped at edit_issue: {error}"))?;
    append_tracker_mutation_audit(
        config,
        TrackerMutationAudit {
            command: "forge rework",
            mutation_type: "issue_edit",
            issue_ref: Some(&source.identifier),
            target: Some(report.title.clone()),
            from_state: Some(source.state.clone()),
            to_state: None,
            reason: "forge human review rework content replacement",
        },
    );

    let content_verified = adapter
        .get_issue(&source.identifier)
        .map_err(|error| format!("forge rework stopped at readback: {error}"))?
        .ok_or_else(|| {
            format!(
                "forge rework stopped at readback: issue disappeared after update: {}",
                source.identifier
            )
        })?;
    if content_verified.title != report.title
        || content_verified.description.as_deref() != Some(input.markdown.as_str())
    {
        return Err(format!(
            "forge rework stopped at readback: expected title {:?} and replacement body for {}, got title {:?}",
            report.title, content_verified.identifier, content_verified.title
        )
        .into());
    }

    let readbacks = vec![format!(
        "`forge rework --write` replaced the issue content; pre-status readback confirmed issue `{}` title `{}` before the final Project status mutation.",
        content_verified.identifier, content_verified.title
    )];
    let workpad = render_forge_rework_workpad(
        &content_verified,
        &content_verified.title,
        &confirmation,
        &evidence,
        &readbacks,
    );
    adapter
        .add_issue_comment(&content_verified.identifier, &workpad)
        .map_err(|error| format!("forge rework stopped at evidence_comment: {error}"))?;
    append_tracker_mutation_audit(
        config,
        TrackerMutationAudit {
            command: "forge rework",
            mutation_type: "timeline_comment",
            issue_ref: Some(&content_verified.identifier),
            target: Some("Rework Revision Evidence".into()),
            from_state: Some(source.state.clone()),
            to_state: None,
            reason: "forge human review rework evidence before status change",
        },
    );

    adapter
        .set_state(&source.identifier, "rework")
        .map_err(|error| format!("forge rework stopped at set_status: {error}"))?;
    append_tracker_mutation_audit(
        config,
        TrackerMutationAudit {
            command: "forge rework",
            mutation_type: "status",
            issue_ref: Some(&source.identifier),
            target: Some("Rework".into()),
            from_state: Some(source.state.clone()),
            to_state: Some("rework".into()),
            reason: "forge human review rework final status update",
        },
    );

    let verified = adapter
        .get_issue(&source.identifier)
        .map_err(|error| format!("forge rework stopped at final_readback: {error}"))?
        .ok_or_else(|| {
            format!(
                "forge rework stopped at final_readback: issue disappeared after update: {}",
                source.identifier
            )
        })?;
    if normalize_state(&verified.state) != normalize_state(&config.tracker.state_map.rework) {
        return Err(format!(
            "forge rework stopped at final_readback: expected Rework, got {:?}",
            verified.state
        )
        .into());
    }

    println!(
        "forge_rework=ok issue={} status=Rework title={:?} evidence=workpad final_status_mutation=true",
        verified.identifier, verified.title
    );
    Ok(())
}

fn clean_rework_text(value: &str, field: &str) -> Result<String, Box<dyn std::error::Error>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(format!("forge rework requires non-empty {field}").into())
    } else {
        Ok(trimmed.to_string())
    }
}

fn ensure_no_active_human_review_lane_claims(
    issue: &TrackerIssue,
) -> Result<(), Box<dyn std::error::Error>> {
    for (field, lane) in [
        ("Main Agent", LaneClaimLane::Main),
        ("Review Agent", LaneClaimLane::Review),
        ("Merging Agent", LaneClaimLane::Merge),
    ] {
        let Some(value) = project_text_field(issue, field) else {
            continue;
        };
        let claim = LaneClaim::parse(&value).map_err(|error| {
            format!(
                "forge rework stopped at preflight: Human Review has unparseable {field} claim: {error}"
            )
        })?;
        if claim.lane != lane {
            return Err(format!(
                "forge rework stopped at preflight: Human Review has mismatched {field} claim lane={}",
                claim.lane.as_str()
            )
            .into());
        }
        if !claim.state.is_terminal_audit_pointer() {
            return Err(format!(
                "forge rework stopped at preflight: Human Review has active {field} claim run={} state={}",
                claim.run,
                claim.state.as_str()
            )
            .into());
        }
    }
    Ok(())
}

fn render_forge_rework_workpad(
    issue: &TrackerIssue,
    rework_title: &str,
    operator_confirmation: &str,
    evidence: &str,
    generated_readbacks: &[String],
) -> String {
    let mut readback_lines = Vec::new();
    push_markdown_bullets(&mut readback_lines, generated_readbacks);
    render_workpad_template(
        None,
        WorkpadTemplateId::ForgeReworkRun,
        &[
            ("generated_at", current_gmt_timestamp()),
            ("issue_ref", issue.identifier.clone()),
            ("issue_title", issue.title.clone()),
            ("pr", timeline_pr_summary(issue)),
            ("rework_title", rework_title.into()),
            (
                "operator_confirmation",
                format!("{operator_confirmation:?}"),
            ),
            ("evidence", evidence.trim().into()),
            ("readbacks", readback_lines.join("\n")),
        ],
    )
}

fn render_forge_rework_blocked_workpad(issue: &TrackerIssue, reason: &str) -> String {
    render_workpad_template(
        None,
        WorkpadTemplateId::ForgeReworkBlocked,
        &[
            ("generated_at", current_gmt_timestamp()),
            ("issue_ref", issue.identifier.clone()),
            ("issue_title", issue.title.clone()),
            ("pr", timeline_pr_summary(issue)),
            ("reason", reason.into()),
        ],
    )
}
