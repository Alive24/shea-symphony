use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Value};
use shea_symphony::config::RuntimeConfig;
use shea_symphony::issue_forge::ForgeValidationReport;
use shea_symphony::issue_templates::load_executable_issue_template;
use shea_symphony::lane_claim::{LaneClaim, LaneClaimLane};
use shea_symphony::model::{TrackerIssue, GITHUB_ISSUE_BODY_FIELD};
use shea_symphony::tracker::{
    adapter_from_config, resolve_configured_tracker_state, IssueRelationshipReadback,
    TrackerAdapter,
};
use shea_symphony::workflow::WorkflowDefinition;
use shea_symphony::workpad_templates::{render_workpad_template, WorkpadTemplateId};

use crate::orchestration::{
    add_timeline_comment_with_recovery, append_tracker_mutation_audit, current_gmt_timestamp,
    preflight_canonical_checkout_for_write_mode, recovery_key, stable_recovery_hash,
    TrackerMutationAudit,
};

use super::{forge_validation_report_for_existing_todo, print_forge_validation};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForgeReviseOptions {
    pub(crate) workflow_path: PathBuf,
    pub(crate) issue_ref: String,
    pub(crate) title: String,
    pub(crate) markdown: String,
    pub(crate) operator_confirmation: Option<String>,
    pub(crate) write: bool,
    pub(crate) dry_run: bool,
}

pub(crate) fn forge_revise(options: ForgeReviseOptions) -> Result<(), Box<dyn std::error::Error>> {
    if options.write && options.dry_run {
        return Err("forge revise cannot use --write and --dry-run together".into());
    }
    let dry_run = !options.write || options.dry_run;
    let workflow = WorkflowDefinition::load(&options.workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &options.workflow_path)?;
    config.validate()?;
    preflight_canonical_checkout_for_write_mode(&config, "forge revise", options.write)?;

    // A canonical fast-forward may have changed repository-owned policy; bind the
    // revision only to a fresh workflow/config read after the write preflight.
    let workflow = WorkflowDefinition::load(&options.workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &options.workflow_path)?;
    config.validate()?;
    let adapter = adapter_from_config(&config);
    forge_revise_with_adapter(
        Some(&workflow),
        &options.workflow_path,
        &config,
        adapter.as_ref(),
        ForgeReviseInput {
            issue_ref: options.issue_ref,
            title: options.title,
            markdown: options.markdown,
            operator_confirmation: options.operator_confirmation,
            dry_run,
        },
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForgeReviseInput {
    pub(crate) issue_ref: String,
    pub(crate) title: String,
    pub(crate) markdown: String,
    pub(crate) operator_confirmation: Option<String>,
    pub(crate) dry_run: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedForgeRevision {
    pub(crate) issue: TrackerIssue,
    pub(crate) source_body: String,
    pub(crate) source_fingerprint: String,
    pub(crate) target_fingerprint: String,
    pub(crate) workflow_fingerprint: String,
    pub(crate) template_fingerprint: String,
    pub(crate) confirmation_token: String,
    preservation: String,
    validation: ForgeValidationReport,
}

#[derive(Debug, Clone)]
struct RevisionFacts {
    issue: TrackerIssue,
    source_body: String,
    source_fingerprint: String,
    workflow_fingerprint: String,
    template_fingerprint: String,
    preservation: String,
}

pub(crate) fn forge_revise_with_adapter(
    workflow: Option<&WorkflowDefinition>,
    workflow_path: &Path,
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    input: ForgeReviseInput,
) -> Result<(), Box<dyn std::error::Error>> {
    let prepared = prepare_forge_revision(
        workflow_path,
        config,
        adapter,
        &input.issue_ref,
        &input.title,
        &input.markdown,
    )?;
    print_forge_validation(&prepared.validation);

    if prepared.issue.title == input.title && prepared.source_body == input.markdown {
        println!(
            "forge_revise=already_applied issue={} status=Todo title={:?} tracker_mutation=false",
            prepared.issue.identifier, prepared.issue.title
        );
        return Ok(());
    }

    if input.dry_run {
        println!(
            "forge_revise_dry_run=would_change issue={} status=Todo title={:?} tracker_mutation=false",
            prepared.issue.identifier, input.title
        );
        println!("confirmation_token={}", prepared.confirmation_token);
        println!("source_fingerprint={}", prepared.source_fingerprint);
        println!("target_fingerprint={}", prepared.target_fingerprint);
        println!("workflow_fingerprint={}", prepared.workflow_fingerprint);
        println!("template_fingerprint={}", prepared.template_fingerprint);
        return Ok(());
    }

    let confirmation = input
        .operator_confirmation
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or("forge revise --write requires --operator-confirmation from the dry-run preview")?;
    if confirmation != prepared.confirmation_token {
        return Err(format!(
            "forge revise stopped at confirmation: expected {:?}, got {:?}; rerun --dry-run",
            prepared.confirmation_token, confirmation
        )
        .into());
    }

    recheck_prepared_revision(workflow_path, config, adapter, &prepared)
        .map_err(|error| format!("forge revise stopped at pre_write_recheck: {error}"))?;

    let evidence = render_revision_evidence(workflow, &prepared)?;
    let evidence_key = recovery_key(
        "forge-revise-evidence",
        &prepared.issue.identifier,
        &prepared.confirmation_token,
    );
    let evidence_outcome = add_timeline_comment_with_recovery(
        adapter,
        &prepared.issue.identifier,
        Some(&prepared.issue),
        &evidence,
        &evidence_key,
        "timeline_comment",
    )
    .map_err(|error| format!("forge revise stopped at revision_evidence: {error}"))?;
    if evidence_outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "forge revise",
                mutation_type: "timeline_comment",
                issue_ref: Some(&prepared.issue.identifier),
                target: Some("Todo Revision Evidence".into()),
                from_state: Some(prepared.issue.state.clone()),
                to_state: None,
                reason: "forge Todo revision evidence before content replacement",
            },
        );
    }
    let evidence_readback = adapter
        .get_issue(&prepared.issue.identifier)
        .map_err(|error| format!("forge revise stopped at evidence_readback: {error}"))?
        .ok_or_else(|| {
            format!(
                "forge revise stopped at evidence_readback: issue disappeared: {}",
                prepared.issue.identifier
            )
        })?;
    if !evidence_readback
        .description
        .as_deref()
        .is_some_and(|description| description.contains(&revision_evidence_marker(&prepared)))
    {
        return Err(
            "forge revise stopped at evidence_readback: prepared revision evidence is missing"
                .into(),
        );
    }

    recheck_prepared_revision(workflow_path, config, adapter, &prepared)
        .map_err(|error| format!("forge revise stopped at immediate_pre_edit_recheck: {error}"))?;

    let edit_outcome = match adapter.update_issue_content(
        &prepared.issue.identifier,
        &input.title,
        &input.markdown,
    ) {
        Ok(()) => "applied",
        Err(write_error) => {
            match verify_revision_readback(
                workflow_path,
                config,
                adapter,
                &prepared,
                &input.title,
                &input.markdown,
            ) {
                Ok(()) => "recovered",
                Err(readback_error) => {
                    let current = read_revision_facts(
                        workflow_path,
                        config,
                        adapter,
                        &prepared.issue.identifier,
                    );
                    if current
                        .as_ref()
                        .is_ok_and(|facts| facts.source_fingerprint == prepared.source_fingerprint)
                    {
                        return Err(format!(
                            "forge revise stopped at edit_issue: not_applied write_error={write_error} readback={readback_error}"
                        )
                        .into());
                    }
                    return Err(format!(
                        "forge revise stopped at edit_issue: ambiguous write_error={write_error} readback={readback_error}"
                    )
                    .into());
                }
            }
        }
    };

    verify_revision_readback(
        workflow_path,
        config,
        adapter,
        &prepared,
        &input.title,
        &input.markdown,
    )?;
    append_tracker_mutation_audit(
        config,
        TrackerMutationAudit {
            command: "forge revise",
            mutation_type: "issue_edit",
            issue_ref: Some(&prepared.issue.identifier),
            target: Some(input.title.clone()),
            from_state: Some(prepared.issue.state.clone()),
            to_state: Some(prepared.issue.state.clone()),
            reason: "forge guarded Todo contract revision",
        },
    );
    println!(
        "forge_revise={edit_outcome} issue={} status=Todo title={:?} evidence=prepared_before_edit preservation=verified",
        prepared.issue.identifier, input.title
    );
    Ok(())
}

pub(crate) fn prepare_forge_revision(
    workflow_path: &Path,
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    title: &str,
    markdown: &str,
) -> Result<PreparedForgeRevision, Box<dyn std::error::Error>> {
    let facts = read_revision_facts(workflow_path, config, adapter, issue_ref)?;
    let validation =
        forge_validation_report_for_existing_todo(title, markdown, config, &facts.issue)
            .map_err(|error| format!("forge revise stopped at validate: {error}"))?;
    if !validation.decision.is_dispatchable() {
        return Err(
            "forge revise stopped at validate: replacement body failed executable issue gate"
                .into(),
        );
    }
    let target_fingerprint = fingerprint_serializable(&json!({
        "issue": facts.issue.identifier,
        "title": title,
        "body": markdown,
    }))?;
    let confirmation_token = format!(
        "todo-revise-{}",
        fingerprint_serializable(&json!({
            "issue": facts.issue.identifier,
            "source": facts.source_fingerprint,
            "target": target_fingerprint,
            "workflow": facts.workflow_fingerprint,
            "template": facts.template_fingerprint,
        }))?
    );
    Ok(PreparedForgeRevision {
        issue: facts.issue,
        source_body: facts.source_body,
        source_fingerprint: facts.source_fingerprint,
        target_fingerprint,
        workflow_fingerprint: facts.workflow_fingerprint,
        template_fingerprint: facts.template_fingerprint,
        confirmation_token,
        preservation: facts.preservation,
        validation,
    })
}

fn read_revision_facts(
    workflow_path: &Path,
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
) -> Result<RevisionFacts, Box<dyn std::error::Error>> {
    let issue = adapter
        .get_issue(issue_ref)
        .map_err(|error| format!("forge revise stopped at read_source: {error}"))?
        .ok_or_else(|| {
            format!("forge revise stopped at read_source: issue not found: {issue_ref}")
        })?;
    validate_revision_source(config, &issue)?;
    let relationships = adapter
        .relationship_readback(&issue.identifier)
        .map_err(|error| format!("forge revise stopped at relationships_read: {error}"))?;
    let linked_pull_requests = adapter
        .list_linked_pull_requests(&issue.identifier)
        .map_err(|error| format!("forge revise stopped at linked_pr_read: {error}"))?;
    let source_body = exact_issue_body(&issue);
    let preservation = preservation_snapshot(&issue, relationships, linked_pull_requests)?;
    let source_fingerprint = fingerprint_serializable(&json!({
        "preservation": preservation,
        "title": issue.title,
        "body": source_body,
    }))?;
    let workflow_fingerprint = fingerprint_file(workflow_path)?;
    load_executable_issue_template(config)?;
    let template_fingerprint = fingerprint_file(&config.issue_templates.executable)?;
    Ok(RevisionFacts {
        issue,
        source_body,
        source_fingerprint,
        workflow_fingerprint,
        template_fingerprint,
        preservation,
    })
}

fn validate_revision_source(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
) -> Result<(), Box<dyn std::error::Error>> {
    let state = resolve_configured_tracker_state(&config.tracker.state_map, &issue.state)
        .map_err(|error| format!("forge revise stopped at preflight: {error}"))?;
    if state.canonical_key() != "todo" {
        return Err(format!(
            "forge revise stopped at preflight: {} is in {:?}, expected Todo",
            issue.identifier, issue.state
        )
        .into());
    }
    match issue
        .project_fields
        .get("GitHub Issue State")
        .and_then(Value::as_str)
    {
        Some(state) if state.eq_ignore_ascii_case("OPEN") => {}
        Some(state) => {
            return Err(format!(
            "forge revise stopped at preflight: {} GitHub issue state is {state:?}, expected OPEN",
            issue.identifier
        )
            .into())
        }
        None if config.tracker.kind == "github_project_v2" => {
            return Err(format!(
                "forge revise stopped at preflight: {} has no authoritative GitHub issue state",
                issue.identifier
            )
            .into())
        }
        None => {}
    }
    ensure_no_active_revision_claims(issue)
}

fn ensure_no_active_revision_claims(
    issue: &TrackerIssue,
) -> Result<(), Box<dyn std::error::Error>> {
    for (field, expected_lane) in [
        ("Main Agent", LaneClaimLane::Main),
        ("Review Agent", LaneClaimLane::Review),
        ("Merging Agent", LaneClaimLane::Merge),
    ] {
        let Some(value) = issue.project_fields.get(field) else {
            continue;
        };
        let Some(value) = value.as_str() else {
            return Err(format!(
                "forge revise stopped at preflight: malformed {field} claim is not text"
            )
            .into());
        };
        if value.trim().is_empty() {
            continue;
        }
        let claim = LaneClaim::parse(value.trim()).map_err(|error| {
            format!("forge revise stopped at preflight: malformed {field} claim: {error}")
        })?;
        if claim.lane != expected_lane || claim.issue != issue.identifier {
            return Err(format!(
                "forge revise stopped at preflight: mismatched {field} claim lane={} issue={}",
                claim.lane.as_str(),
                claim.issue
            )
            .into());
        }
        if !claim.state.is_terminal_audit_pointer() {
            return Err(format!(
                "forge revise stopped at preflight: active {field} claim run={} state={}",
                claim.run,
                claim.state.as_str()
            )
            .into());
        }
    }
    Ok(())
}

fn recheck_prepared_revision(
    workflow_path: &Path,
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    prepared: &PreparedForgeRevision,
) -> Result<(), Box<dyn std::error::Error>> {
    let observed = read_revision_facts(workflow_path, config, adapter, &prepared.issue.identifier)?;
    for (name, expected, actual) in [
        (
            "source",
            prepared.source_fingerprint.as_str(),
            observed.source_fingerprint.as_str(),
        ),
        (
            "workflow",
            prepared.workflow_fingerprint.as_str(),
            observed.workflow_fingerprint.as_str(),
        ),
        (
            "template",
            prepared.template_fingerprint.as_str(),
            observed.template_fingerprint.as_str(),
        ),
    ] {
        if expected != actual {
            return Err(format!(
                "{name} fingerprint drifted expected={expected} observed={actual}; rerun --dry-run"
            )
            .into());
        }
    }
    Ok(())
}

fn verify_revision_readback(
    workflow_path: &Path,
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    prepared: &PreparedForgeRevision,
    title: &str,
    markdown: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let observed = read_revision_facts(workflow_path, config, adapter, &prepared.issue.identifier)
        .map_err(|error| format!("forge revise stopped at final_readback: {error}"))?;
    if observed.issue.title != title || observed.source_body != markdown {
        return Err(format!(
            "forge revise stopped at final_readback: expected exact title/body, got title {:?}",
            observed.issue.title
        )
        .into());
    }
    if observed.preservation != prepared.preservation {
        return Err(
            "forge revise stopped at final_readback: preserved tracker facts changed".into(),
        );
    }
    Ok(())
}

fn preservation_snapshot(
    issue: &TrackerIssue,
    mut relationships: IssueRelationshipReadback,
    linked_pull_requests: Vec<shea_symphony::model::LinkedPullRequest>,
) -> Result<String, Box<dyn std::error::Error>> {
    relationships.blocked_by.sort_by_key(json_sort_key);
    relationships.native_subissues.sort_by_key(json_sort_key);
    let mut linked = linked_pull_requests
        .into_iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()?;
    linked.sort_by_key(|value| value.to_string());
    let mut labels = issue.labels.clone();
    labels.sort();
    let mut assignees = issue.assignees.clone();
    assignees.sort();
    let mut project_fields = issue.project_fields.clone();
    project_fields.remove(GITHUB_ISSUE_BODY_FIELD);
    Ok(serde_json::to_string(&json!({
        "tracker_kind": issue.tracker_kind,
        "id": issue.id,
        "item_id": issue.item_id,
        "identifier": issue.identifier,
        "url": issue.url,
        "state": issue.state,
        "labels": labels,
        "assignees": assignees,
        "priority": issue.priority,
        "branch_name": issue.branch_name,
        "project_fields": project_fields,
        "relationships": relationships,
        "linked_pull_requests": linked,
    }))?)
}

fn exact_issue_body(issue: &TrackerIssue) -> String {
    if let Some(body) = issue
        .project_fields
        .get(GITHUB_ISSUE_BODY_FIELD)
        .and_then(Value::as_str)
    {
        return body.to_string();
    }
    let description = issue.description.as_deref().unwrap_or_default();
    [
        "<!-- shea-symphony-workpad -->",
        "## Shea Symphony Todo Revision",
    ]
    .iter()
    .filter_map(|marker| description.find(marker))
    .min()
    .map(|index| description[..index].trim_end().to_string())
    .unwrap_or_else(|| description.to_string())
}

fn fingerprint_file(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    Ok(stable_recovery_hash(&std::fs::read_to_string(path)?))
}

fn fingerprint_serializable(value: &impl Serialize) -> Result<String, Box<dyn std::error::Error>> {
    Ok(stable_recovery_hash(&serde_json::to_string(value)?))
}

fn json_sort_key(value: &impl Serialize) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn render_revision_evidence(
    workflow: Option<&WorkflowDefinition>,
    prepared: &PreparedForgeRevision,
) -> Result<String, Box<dyn std::error::Error>> {
    Ok(render_workpad_template(
        workflow,
        WorkpadTemplateId::ForgeRevision,
        &[
            ("generated_at", current_gmt_timestamp()),
            ("issue_ref", prepared.issue.identifier.clone()),
            ("issue_title", prepared.issue.title.clone()),
            ("confirmation_token", prepared.confirmation_token.clone()),
            ("source_fingerprint", prepared.source_fingerprint.clone()),
            ("target_fingerprint", prepared.target_fingerprint.clone()),
            (
                "workflow_fingerprint",
                prepared.workflow_fingerprint.clone(),
            ),
            (
                "template_fingerprint",
                prepared.template_fingerprint.clone(),
            ),
        ],
    )?)
}

fn revision_evidence_marker(prepared: &PreparedForgeRevision) -> String {
    format!(
        "<!-- shea-symphony-todo-revision target={} confirmation={} -->",
        prepared.target_fingerprint, prepared.confirmation_token
    )
}
