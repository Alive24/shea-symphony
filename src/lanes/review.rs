use std::path::PathBuf;

use jade_symphony::config::RuntimeConfig;
use jade_symphony::lane_claim::{
    LaneClaim, LaneClaimActor, LaneClaimLane, LaneClaimSource, LaneClaimState,
};
use jade_symphony::model::{normalize_state, TrackerIssue};
use jade_symphony::progress::run_with_progress_heartbeat;
use jade_symphony::review::{
    classify_review_freshness, render_review_freshness_workpad, review_pass_target_state,
    ReviewFreshnessInput,
};
use jade_symphony::review_status::{
    load_review_status, render_review_status_human, ReviewStatusOptions,
};
use jade_symphony::session_registry::unix_timestamp_ms;
use jade_symphony::tracker::{adapter_from_config, ProjectFieldAssignment, TrackerAdapter};
use jade_symphony::workflow::WorkflowDefinition;

use crate::{
    add_timeline_comment_with_recovery, append_tracker_mutation_audit, current_gmt_timestamp,
    hydrate_issues_for_review_lane, lane_claim_for_issue, load_config,
    preflight_canonical_checkout_for_write_mode, progress_spec_for_config, project_text_field,
    record_manual_lane_claim_evidence, recovery_key, render_parseable_lane_claim,
    set_project_field_with_recovery, set_state_with_recovery, stable_recovery_hash,
    timeline_claim_actor, timeline_claim_run, timeline_pr_summary, tracker_backend_label,
    AgentSessionLaneArg, TrackerMutationAudit,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReviewStatusCliOptions {
    pub(crate) workflow_path: PathBuf,
    pub(crate) issue_filter: Option<String>,
    pub(crate) recent_limit: usize,
    pub(crate) verbose: bool,
    pub(crate) json: bool,
}

pub(crate) fn review_freshness(
    input: ReviewFreshnessInput,
) -> Result<(), Box<dyn std::error::Error>> {
    let report = classify_review_freshness(input);
    println!("review_freshness={:?}", report.decision.kind);
    println!(
        "prior_human_review_valid={}",
        report.decision.prior_human_review_valid
    );
    println!(
        "human_rereview_required={}",
        report.decision.human_rereview_required
    );
    println!(
        "main_agent_target_state={}",
        report.decision.main_agent_target_state
    );
    println!(
        "authorized_next_state={}",
        report
            .decision
            .authorized_next_state
            .as_deref()
            .unwrap_or("none")
    );
    println!("rationale={}", report.decision.rationale);
    println!("\n--- review freshness evidence ---\n");
    println!("{}", render_review_freshness_workpad(&report));
    Ok(())
}

pub(crate) fn review_status(
    options: ReviewStatusCliOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let workflow = WorkflowDefinition::load(&options.workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &options.workflow_path)?;
    config.validate()?;
    let adapter = adapter_from_config(&config);
    let issues = if let Some(issue_ref) = &options.issue_filter {
        adapter
            .get_issue(issue_ref)?
            .map(|issue| vec![issue])
            .ok_or_else(|| format!("issue not found: {issue_ref}"))?
    } else {
        let mut states = config.tracker.active_states.clone();
        if !states.iter().any(|state| {
            normalize_state(state) == normalize_state(&config.tracker.state_map.agent_review)
        }) {
            states.push(config.tracker.state_map.agent_review.clone());
        }
        hydrate_issues_for_review_lane(adapter.as_ref(), adapter.fetch_issues_by_states(&states)?)?
    };
    let payload = load_review_status(
        &config,
        &issues,
        &ReviewStatusOptions {
            issue_filter: options.issue_filter.clone(),
            recent_limit: options.recent_limit,
            verbose: options.verbose,
        },
        unix_timestamp_ms(),
    )?;

    if options.json {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("{}", render_review_status_human(&payload, options.verbose));
    }
    Ok(())
}

pub(crate) fn review_claim(
    workflow_path: PathBuf,
    issue_ref: String,
    worker: String,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    preflight_canonical_checkout_for_write_mode(&config, "review claim", write)?;
    let adapter = adapter_from_config(&config);
    let issue = adapter
        .get_issue(&issue_ref)?
        .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    if issue.normalized_state() != "agent review" {
        return Err(format!(
            "review claim requires Agent Review state; {} is currently {}",
            issue.identifier, issue.state
        )
        .into());
    }
    let claim = lane_claim_for_issue(
        &issue,
        LaneClaimLane::Review,
        if worker.to_ascii_lowercase().contains("gemini") {
            LaneClaimActor::Gemini
        } else {
            LaneClaimActor::Codex
        },
        LaneClaimSource::Manual,
        project_text_field(&issue, "Review Agent").as_deref(),
    )
    .with_worker(&worker);
    let claim_value = render_parseable_lane_claim(&claim)?;
    if !write {
        println!(
            "review_claim_dry_run action=claim_field issue_ref={} field=\"Review Agent\" value={claim_value}",
            issue.identifier
        );
        return Ok(());
    }
    let outcome = set_project_field_with_recovery(
        adapter.as_ref(),
        &issue,
        &ProjectFieldAssignment {
            name: "Review Agent".into(),
            value: claim_value.clone(),
        },
        "claim_field",
    )?;
    let registry_path = record_manual_lane_claim_evidence(
        &config,
        &issue,
        AgentSessionLaneArg::Review,
        &claim,
        &claim_value,
        &worker,
    )?;
    if outcome.should_record_audit() {
        append_tracker_mutation_audit(
            &config,
            TrackerMutationAudit {
                command: "review claim",
                mutation_type: "claim_field",
                issue_ref: Some(&issue.identifier),
                target: Some(format!("Review Agent={claim_value}")),
                from_state: Some(issue.state.clone()),
                to_state: None,
                reason: "manual review agent claim",
            },
        );
    }
    println!(
        "review_claim={} issue_ref={} field=\"Review Agent\" run={} registry={} value={claim_value}",
        outcome.as_str(),
        issue.identifier,
        claim.run,
        registry_path.display()
    );
    Ok(())
}

pub(crate) fn review_clear_claim(
    workflow_path: PathBuf,
    issue_ref: String,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let issue = run_with_progress_heartbeat(
        progress_spec_for_config(&config, "github_project_read")
            .issue(issue_ref.clone())
            .backend(tracker_backend_label(&config))
            .next("inspect_issue"),
        || adapter.get_issue(&issue_ref),
    )?
    .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    if !write {
        println!(
            "review_clear_claim_dry_run action=clear_claim_field issue_ref={} field=\"Review Agent\"",
            issue.identifier
        );
        return Ok(());
    }
    adapter.clear_project_field(&issue.identifier, "Review Agent")?;
    append_tracker_mutation_audit(
        &config,
        TrackerMutationAudit {
            command: "review-clear-claim",
            mutation_type: "claim_field_clear",
            issue_ref: Some(&issue.identifier),
            target: Some("Review Agent".into()),
            from_state: Some(issue.state),
            to_state: None,
            reason: "manual review agent claim clear",
        },
    );
    println!(
        "review_clear_claim=ok issue_ref={} field=\"Review Agent\"",
        issue.identifier
    );
    Ok(())
}

pub(crate) fn review_manual_pass(
    workflow_path: PathBuf,
    issue_ref: String,
    evidence: String,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    preflight_canonical_checkout_for_write_mode(&config, "review pass", write)?;
    let adapter = adapter_from_config(&config);
    let issue = adapter
        .get_issue(&issue_ref)?
        .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    if issue.normalized_state() != "agent review" {
        return Err(format!(
            "manual review pass requires Agent Review state; {} is currently {}",
            issue.identifier, issue.state
        )
        .into());
    }

    let (current_claim_value, current_claim) =
        validate_manual_review_pass_claim(&issue, &evidence)?;
    let terminal_claim_value =
        terminal_review_claim_value(&current_claim, LaneClaimState::Done, "passed");
    let target_state = review_pass_target_state(&issue);
    let workpad = render_manual_review_workpad(
        &issue,
        "passed",
        target_state,
        &evidence,
        true,
        &current_claim_value,
        &terminal_claim_value,
    );
    if !write {
        println!(
            "review_pass_dry_run action=timeline_comment issue_ref={} evidence=manual_review_pass",
            issue.identifier
        );
        println!(
            "review_pass_dry_run action=update_claim_field issue_ref={} field=\"Review Agent\" value={terminal_claim_value}",
            issue.identifier,
        );
        println!(
            "review_pass_dry_run action=set_state issue_ref={} target_state={target_state}",
            issue.identifier
        );
        return Ok(());
    }
    let evidence_key = recovery_key(
        "review-pass",
        &issue.identifier,
        &format!(
            "{}|{}|{}",
            terminal_claim_value,
            target_state,
            stable_recovery_hash(&workpad)
        ),
    );
    let evidence_outcome = add_timeline_comment_with_recovery(
        adapter.as_ref(),
        &issue.identifier,
        Some(&issue),
        &workpad,
        &evidence_key,
        "timeline_comment",
    )?;
    if evidence_outcome.should_record_audit() {
        append_tracker_mutation_audit(
            &config,
            TrackerMutationAudit {
                command: "review pass",
                mutation_type: "timeline_comment",
                issue_ref: Some(&issue.identifier),
                target: None,
                from_state: Some(issue.state.clone()),
                to_state: Some(target_state.into()),
                reason: "manual review pass evidence",
            },
        );
    }
    write_terminal_review_claim(
        &config,
        adapter.as_ref(),
        &issue.identifier,
        &issue.state,
        &terminal_claim_value,
        "review pass terminal claim evidence",
    )?;
    let state_outcome = set_state_with_recovery(
        adapter.as_ref(),
        &issue.identifier,
        Some(&issue),
        target_state,
        "state_change",
    )?;
    if state_outcome.should_record_audit() {
        append_tracker_mutation_audit(
            &config,
            TrackerMutationAudit {
                command: "review pass",
                mutation_type: "state_change",
                issue_ref: Some(&issue.identifier),
                target: None,
                from_state: Some(issue.state.clone()),
                to_state: Some(target_state.into()),
                reason: "manual review pass routing",
            },
        );
    }
    println!(
        "review_pass=ok issue_ref={} target_state={target_state}",
        issue.identifier
    );
    Ok(())
}

pub(crate) fn review_manual_reject(
    workflow_path: PathBuf,
    issue_ref: String,
    evidence: String,
    target_state: String,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let normalized_target = normalize_state(&target_state);
    if normalized_target == "human_review" {
        return Err("review reject cannot target Human Review".into());
    }
    if !matches!(
        normalized_target.as_str(),
        "agent_review" | "agent review" | "rework" | "need_human_input" | "need human input"
    ) {
        return Err(
            "review reject target must be agent_review, rework, or need_human_input".into(),
        );
    }

    let config = load_config(&workflow_path)?;
    preflight_canonical_checkout_for_write_mode(&config, "review reject", write)?;
    let adapter = adapter_from_config(&config);
    let issue = adapter
        .get_issue(&issue_ref)?
        .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    if issue.normalized_state() != "agent review" {
        return Err(format!(
            "manual review reject requires Agent Review state; {} is currently {}",
            issue.identifier, issue.state
        )
        .into());
    }

    let (current_claim_value, current_claim) =
        validate_active_manual_review_claim(&issue, &evidence)?;
    let (terminal_state, terminal_result) = reject_terminal_claim_outcome(&normalized_target);
    let terminal_claim_value =
        terminal_review_claim_value(&current_claim, terminal_state, terminal_result);
    let workpad = render_manual_review_workpad(
        &issue,
        "not passed",
        &target_state,
        &evidence,
        false,
        &current_claim_value,
        &terminal_claim_value,
    );
    if !write {
        println!(
            "review_reject_dry_run action=timeline_comment issue_ref={} evidence=manual_review_reject",
            issue.identifier
        );
        println!(
            "review_reject_dry_run action=update_claim_field issue_ref={} field=\"Review Agent\" value={terminal_claim_value}",
            issue.identifier,
        );
        println!(
            "review_reject_dry_run action=set_state issue_ref={} target_state={target_state}",
            issue.identifier
        );
        return Ok(());
    }
    let evidence_key = recovery_key(
        "review-reject",
        &issue.identifier,
        &format!(
            "{}|{}|{}",
            terminal_claim_value,
            target_state,
            stable_recovery_hash(&workpad)
        ),
    );
    let evidence_outcome = add_timeline_comment_with_recovery(
        adapter.as_ref(),
        &issue.identifier,
        Some(&issue),
        &workpad,
        &evidence_key,
        "timeline_comment",
    )?;
    if evidence_outcome.should_record_audit() {
        append_tracker_mutation_audit(
            &config,
            TrackerMutationAudit {
                command: "review reject",
                mutation_type: "timeline_comment",
                issue_ref: Some(&issue.identifier),
                target: None,
                from_state: Some(issue.state.clone()),
                to_state: Some(target_state.clone()),
                reason: "manual review reject evidence",
            },
        );
    }
    write_terminal_review_claim(
        &config,
        adapter.as_ref(),
        &issue.identifier,
        &issue.state,
        &terminal_claim_value,
        "review reject terminal claim evidence",
    )?;
    let state_outcome = set_state_with_recovery(
        adapter.as_ref(),
        &issue.identifier,
        Some(&issue),
        &target_state,
        "state_change",
    )?;
    if state_outcome.should_record_audit() {
        append_tracker_mutation_audit(
            &config,
            TrackerMutationAudit {
                command: "review reject",
                mutation_type: "state_change",
                issue_ref: Some(&issue.identifier),
                target: None,
                from_state: Some(issue.state.clone()),
                to_state: Some(target_state.clone()),
                reason: "manual review reject routing",
            },
        );
    }
    println!(
        "review_reject=ok issue_ref={} target_state={target_state}",
        issue.identifier
    );
    Ok(())
}

pub(crate) fn render_manual_review_workpad(
    issue: &TrackerIssue,
    decision: &str,
    target_state: &str,
    evidence: &str,
    pass: bool,
    current_claim_value: &str,
    terminal_claim_value: &str,
) -> String {
    let mut lines = vec![
        "## Jade Symphony Agent Review Run".to_string(),
        String::new(),
        format!("- Generated at: `{}`", current_gmt_timestamp()),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        "- Lane: `review`".into(),
        "- Actor role: `review_agent`".into(),
        format!(
            "- Actor: `{}`",
            timeline_claim_actor(current_claim_value).unwrap_or("manual-operator".into())
        ),
        format!(
            "- Run ID: `{}`",
            timeline_claim_run(current_claim_value).unwrap_or("not recorded".into())
        ),
        "- Input state: `Agent Review`".into(),
        "- Reviewer backend: manual-operator".into(),
        format!("- Decision: Manual independent review {decision}."),
        format!("- Target state after review routing: `{target_state}`"),
        format!("- Result: `{}`", if pass { "passed" } else { "rework" }),
        format!("- PR: `{}`", timeline_pr_summary(issue)),
        format!("- Review Agent claim: `{current_claim_value}`"),
        format!("- Terminal Review Agent claim: `{terminal_claim_value}`"),
        "- Evidence summary: manual review evidence captured below.".into(),
        String::new(),
        "### Manual Review Evidence".into(),
        "````md".into(),
    ];
    lines.extend(evidence.trim().lines().map(str::to_string));
    lines.push("````".into());
    if pass {
        lines.push(String::new());
        lines.push("- Review pass evidence: `recorded`".into());
        if normalize_state(target_state) == "merging" {
            lines.push("Evidence recorded. Independent Review Agent may move this native subissue directly to Merging; final Human Review and UAT remain owned by the parent issue.".into());
        } else {
            lines.push("Evidence recorded. Independent Review Agent may move this issue to Human Review; the main implementation agent must not.".into());
        }
    } else {
        lines.push(String::new());
        lines.push(
            "- Review did not pass; unavailable or inconclusive review must not move to Human Review."
                .into(),
        );
    }
    lines.join("\n")
}

pub(crate) fn validate_manual_review_pass_claim(
    issue: &TrackerIssue,
    evidence: &str,
) -> Result<(String, LaneClaim), Box<dyn std::error::Error>> {
    let (current, claim) = parse_manual_review_claim(issue)?;
    if claim.state == LaneClaimState::Active
        || (claim.state == LaneClaimState::Done
            && review_claim_result_value(&current) == Some("passed"))
    {
        validate_manual_review_evidence_contains_claim(&current, evidence)?;
        return Ok((current, claim));
    }
    Err(format!(
        "current Review Agent claim must be active, or already state=done result=passed for idempotent pass repair; found state={} result={}",
        claim.state.as_str(),
        review_claim_result_value(&current).unwrap_or("missing")
    )
    .into())
}

pub(crate) fn validate_active_manual_review_claim(
    issue: &TrackerIssue,
    evidence: &str,
) -> Result<(String, LaneClaim), Box<dyn std::error::Error>> {
    let (current, claim) = parse_manual_review_claim(issue)?;
    if claim.state != LaneClaimState::Active {
        return Err(format!(
            "current Review Agent claim must be active before routing, found state={}",
            claim.state.as_str()
        )
        .into());
    }
    validate_manual_review_evidence_contains_claim(&current, evidence)?;
    Ok((current, claim))
}

fn parse_manual_review_claim(
    issue: &TrackerIssue,
) -> Result<(String, LaneClaim), Box<dyn std::error::Error>> {
    let current = project_text_field(issue, "Review Agent")
        .ok_or("manual review routing requires a current Review Agent claim")?;
    let claim = LaneClaim::parse(&current).map_err(|error| {
        format!("current Review Agent claim is not a structured lane claim: {error}")
    })?;
    if claim.lane != LaneClaimLane::Review {
        return Err("current Review Agent claim is not for the review lane".into());
    }
    if claim.issue != issue.identifier {
        return Err(format!(
            "current Review Agent claim issue {} does not match {}",
            claim.issue, issue.identifier
        )
        .into());
    }
    Ok((current, claim))
}

fn validate_manual_review_evidence_contains_claim(
    current: &str,
    evidence: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if !evidence.contains(current) {
        return Err(
            "manual review evidence must include the exact current Review Agent claim value".into(),
        );
    }
    Ok(())
}

fn review_claim_result_value(value: &str) -> Option<&str> {
    value
        .split_whitespace()
        .find_map(|token| token.strip_prefix("result="))
}

pub(crate) fn terminal_review_claim_value(
    claim: &LaneClaim,
    state: LaneClaimState,
    result: &str,
) -> String {
    format!("{} result={result}", claim.with_state(state).render())
}

fn reject_terminal_claim_outcome(normalized_target: &str) -> (LaneClaimState, &'static str) {
    match normalized_target {
        "rework" => (LaneClaimState::Done, "rejected"),
        "need_human_input" | "need human input" => (LaneClaimState::Failed, "blocked"),
        _ => (LaneClaimState::Failed, "inconclusive"),
    }
}

pub(crate) fn write_terminal_review_claim(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    from_state: &str,
    value: &str,
    reason: &'static str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut issue = adapter
        .get_issue(issue_ref)?
        .ok_or_else(|| format!("issue not found before terminal review claim: {issue_ref}"))?;
    issue.state = from_state.into();
    let outcome = set_project_field_with_recovery(
        adapter,
        &issue,
        &ProjectFieldAssignment {
            name: "Review Agent".into(),
            value: value.into(),
        },
        "claim_field",
    )?;
    if outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "manual review routing",
                mutation_type: "claim_field",
                issue_ref: Some(issue_ref),
                target: Some(format!("Review Agent={value}")),
                from_state: Some(from_state.into()),
                to_state: None,
                reason,
            },
        );
    }
    Ok(())
}
