use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use jade_symphony::config::RuntimeConfig;
use jade_symphony::issue_forge::{next_clarification_question, ForgeValidationReport};
use jade_symphony::lane_claim::{LaneClaim, LaneClaimLane};
use jade_symphony::model::{normalize_state, GateDecision, GateDecisionKind, TrackerIssue};
use jade_symphony::tracker::{
    adapter_from_config, FollowUpIssueInput, ProjectFieldAssignment, TrackerAdapter,
};

use crate::cli::ForgeStatusArg;
use crate::{
    append_tracker_mutation_audit, current_gmt_timestamp, evaluate_issue_for_current_source,
    load_config, preflight_canonical_checkout_for_write_mode, project_text_field,
    timeline_pr_summary, TrackerMutationAudit,
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
    let report = forge_validation_report(status, &title, &markdown, &config, &assignees)?;
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

    adapter.add_issue_to_project_with_state(&issue_id, input.status.normalized_state())?;
    append_tracker_mutation_audit(
        config,
        TrackerMutationAudit {
            command: "forge create",
            mutation_type: "project_add",
            issue_ref: Some(&issue_id),
            target: Some(input.project_label.into()),
            from_state: None,
            to_state: Some(input.status.normalized_state().into()),
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
    let mut lines = vec![
        "## Jade Symphony Rework Run".to_string(),
        String::new(),
        format!("- Generated at: `{}`", current_gmt_timestamp()),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        "- Lane: `main`".into(),
        "- Actor role: `human_review_revision`".into(),
        "- Actor: `operator`".into(),
        "- Run ID: `forge-rework`".into(),
        "- Run type: `human_review_rework_revision`".into(),
        "- Input state: `Human Review`".into(),
        "- Target state after run: `Rework`".into(),
        "- Result: `rework_revision_recorded`".into(),
        format!("- PR: `{}`", timeline_pr_summary(issue)),
        format!("- Replacement Rework title/status: `{rework_title}` / `Rework`"),
        format!("- Operator confirmation: {operator_confirmation:?}"),
        "- Evidence summary: operator confirmation, replacement contract, and readback evidence recorded.".into(),
        "- Source state validated as `Human Review` before mutation.".into(),
        "- Terminal lane claims, when present, were preserved as audit pointers.".into(),
        "- Active lane claims in `Human Review` are rejected before content or status writes."
            .into(),
        "- Replacement body was written and read back before the final Project status mutation."
            .into(),
        "- Final Project status mutation is `Rework`.".into(),
        String::new(),
        "### Rework Direction".into(),
        String::new(),
        evidence.trim().to_string(),
        String::new(),
        "### Verification Readback".into(),
        String::new(),
    ];
    push_markdown_bullets(&mut lines, generated_readbacks);
    lines.extend([
        String::new(),
        "### Role Boundary".into(),
        String::new(),
        "- Main Agent may claim `Rework`, repair the revised contract, and stop at `Agent Review`."
            .into(),
        "- `Human Review` remains reserved for independent Review Agent pass evidence.".into(),
    ]);
    lines.join("\n")
}

fn render_forge_rework_blocked_workpad(issue: &TrackerIssue, reason: &str) -> String {
    [
        "## Jade Symphony Rework Run".to_string(),
        String::new(),
        format!("- Generated at: `{}`", current_gmt_timestamp()),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        "- Lane: `main`".into(),
        "- Actor role: `human_review_revision`".into(),
        "- Actor: `operator`".into(),
        "- Run ID: `forge-rework`".into(),
        "- Run type: `human_review_rework_revision`".into(),
        "- Source state: `Human Review`".into(),
        "- Target state after run: `unchanged`".into(),
        "- Result: `blocked`".into(),
        format!("- PR: `{}`", timeline_pr_summary(issue)),
        format!("- Blocker: {reason}"),
        "- Evidence summary: blocked rework revision recorded before any state mutation.".into(),
        "- No replacement body was written.".into(),
        "- Project status was not changed to `Rework`.".into(),
        "- Resolve or supersede the active lane claim before retrying `forge rework`.".into(),
    ]
    .join("\n")
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

fn forge_status_from_issue(config: &RuntimeConfig, issue: &TrackerIssue) -> ForgeStatusArg {
    if normalize_state(&issue.state) == normalize_state(&config.tracker.state_map.backlog) {
        ForgeStatusArg::Backlog
    } else {
        ForgeStatusArg::Todo
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
) -> Result<jade_symphony::issue_forge::ForgeValidationReport, String> {
    let report =
        validate_forge_create_report_with_assignees(title, markdown, config, intended_assignees)
            .map_err(|error| format!("source alignment failed: {error}"))?;
    if report.decision.is_dispatchable() {
        Ok(report)
    } else {
        Err("issue forge validation failed; tracker issue was not created".into())
    }
}

pub(crate) fn validate_forge_create_report_with_assignees(
    title: &str,
    markdown: &str,
    config: &RuntimeConfig,
    intended_assignees: &[String],
) -> Result<jade_symphony::issue_forge::ForgeValidationReport, Box<dyn std::error::Error>> {
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
        blocked_by: Vec::new(),
        project_fields: Default::default(),
        created_at: None,
        updated_at: None,
    };
    let decision = evaluate_issue_for_current_source(config, &issue)?;
    Ok(jade_symphony::issue_forge::ForgeValidationReport {
        title: title.to_string(),
        question: next_clarification_question(&decision),
        decision,
    })
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
