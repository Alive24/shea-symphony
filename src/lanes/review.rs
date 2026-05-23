use std::collections::BTreeMap;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use jade_symphony::config::RuntimeConfig;
use jade_symphony::issue_workspace::{discover_issue_workspaces, WorkspaceMatchStrength};
use jade_symphony::lane_claim::{
    LaneClaim, LaneClaimActor, LaneClaimLane, LaneClaimSource, LaneClaimState,
};
use jade_symphony::model::{normalize_state, TrackerIssue};
use jade_symphony::progress::{run_with_progress_heartbeat, ProgressHeartbeatSpec};
use jade_symphony::prompt::render_prompt;
use jade_symphony::review::{
    classify_review_freshness, gemini_cli_headless_args, gemini_prelaunch_health_diagnostic,
    gemini_review_health_diagnostic, poll_review_job_until_terminal,
    render_repeated_review_failure_workpad, render_review_freshness_workpad, render_review_workpad,
    review_failure_signature, review_gate_decision_for_issue, review_pass_target_state,
    review_run_eligibility, review_worker_key, transition_allowed_for_review_agent,
    write_review_job_ledger_record, FakeReviewBackend, FakeReviewOutcome, GeminiCliReviewBackend,
    GeminiReviewRecoveryPolicy, ReviewBackend, ReviewFreshnessInput, ReviewGateDecision, ReviewJob,
    ReviewJobState, ReviewOutcome, ReviewRepeatedFailureEvidence, ReviewRequest,
    ReviewRunEligibility,
};
use jade_symphony::review_status::{
    load_review_status, render_review_status_human, ReviewStatusOptions,
};
use jade_symphony::rework::rework_transition_expected;
#[cfg(test)]
use jade_symphony::rework::{render_rework_diagnostic_workpad, ReworkDiagnostic};
use jade_symphony::session_registry::unix_timestamp_ms;
use jade_symphony::tracker::{adapter_from_config, ProjectFieldAssignment, TrackerAdapter};
use jade_symphony::workflow::{AgentLane, WorkflowDefinition};

use crate::{
    add_timeline_comment_with_recovery, append_tracker_mutation_audit, current_gmt_timestamp,
    hydrate_issues_for_review_lane, lane_claim_for_issue, latest_status_for_issue, load_config,
    preflight_canonical_checkout_for_write_mode, print_latest_status, progress_spec_for_config,
    progress_spec_with_event_log, project_text_field, record_manual_lane_claim_evidence,
    recovery_key, render_parseable_lane_claim, require_write_intent, run_loop_handoff_plan,
    set_project_field_with_recovery, set_state_with_recovery, shell_quote_display,
    stable_recovery_hash, timeline_claim_actor, timeline_claim_run, timeline_pr_summary,
    tracker_backend_label, unbounded_loop_sleep_ms, AgentSessionLaneArg, TrackerMutationAudit,
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

pub(crate) fn review_fake(
    workflow_path: PathBuf,
    issue_ref: String,
    outcome: FakeReviewOutcome,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    require_write_intent(write)?;
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;
    let adapter = adapter_from_config(&config);
    let issue = run_with_progress_heartbeat(
        progress_spec_with_event_log(&config, "github_project_read")
            .issue(issue_ref.clone())
            .backend(tracker_backend_label(&config))
            .next("review_issue_read"),
        || adapter.get_issue(&issue_ref),
    )?
    .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    let request = ReviewRequest {
        issue: issue.clone(),
        prompt: render_automatic_review_prompt(&workflow, &issue)?,
        workspace: config.workspace.root.clone(),
        artifact_root: config.observability.logs_root.join("reviews"),
    };
    let backend = FakeReviewBackend::new(outcome);
    let job = backend.poll(backend.start(request)?)?;
    apply_review_result(
        &config,
        adapter.as_ref(),
        &issue_ref,
        &issue,
        &job,
        None,
        None,
    )?;

    let decision = review_gate_decision_for_issue(&job, &issue);
    println!(
        "review_fake=ok issue_ref={issue_ref} outcome={:?} target_state={:?}",
        decision.outcome, decision.target_state
    );
    println!("{}", decision.message);
    Ok(())
}

pub(crate) fn review_once(
    workflow_path: PathBuf,
    issue_ref: String,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    require_write_intent(write)?;
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;
    let adapter = adapter_from_config(&config);
    let issue = adapter
        .get_issue(&issue_ref)?
        .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    let request = ReviewRequest {
        issue: issue.clone(),
        prompt: render_automatic_review_prompt(&workflow, &issue)?,
        workspace: config.workspace.root.clone(),
        artifact_root: config.observability.logs_root.join("reviews"),
    };
    let job = match config.review.backend.as_str() {
        "gemini-cli" => {
            let backend = GeminiCliReviewBackend::with_headless_options(
                config.review.gemini_command.clone(),
                config.review.gemini_model.clone(),
                config.review.gemini_allowed_tools.clone(),
            );
            match backend.start(request) {
                Ok(job) => {
                    let spec = review_backend_progress_spec(&config, &issue, backend.kind(), &job);
                    run_with_progress_heartbeat(spec, || {
                        poll_review_job_until_terminal(
                            &backend,
                            job,
                            Duration::from_millis(config.review.timeout_ms),
                            Duration::from_millis(500),
                        )
                    })?
                }
                Err(error) => ReviewJob::failed_unavailable(
                    issue.identifier.clone(),
                    "gemini-cli",
                    error.to_string(),
                ),
            }
        }
        _ => {
            let backend = FakeReviewBackend::new(FakeReviewOutcome::Pass);
            backend.poll(backend.start(request)?)?
        }
    };
    apply_review_result(
        &config,
        adapter.as_ref(),
        &issue_ref,
        &issue,
        &job,
        None,
        None,
    )?;

    let decision = review_gate_decision_for_issue(&job, &issue);
    println!(
        "review_once=ok issue_ref={issue_ref} backend={} outcome={:?} target_state={:?}",
        job.backend, decision.outcome, decision.target_state
    );
    println!("{}", decision.message);
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReviewLoopFailureMemory {
    signature: String,
    first_job_id: String,
    previous_job_id: String,
    repeat_count: usize,
}

fn review_loop_repeated_failure_evidence(
    memory: &mut BTreeMap<String, ReviewLoopFailureMemory>,
    issue: &TrackerIssue,
    job: &ReviewJob,
) -> Option<ReviewRepeatedFailureEvidence> {
    if !matches!(job.state, ReviewJobState::Failed | ReviewJobState::TimedOut) {
        memory.remove(&review_worker_key(issue, &job.backend));
        return None;
    }

    let worker_key = review_worker_key(issue, &job.backend);
    let signature = review_failure_signature(job)?;
    match memory.get_mut(&worker_key) {
        Some(previous) if previous.signature == signature => {
            previous.repeat_count = previous.repeat_count.saturating_add(1);
            let evidence = ReviewRepeatedFailureEvidence {
                repeat_count: previous.repeat_count,
                first_job_id: previous.first_job_id.clone(),
                previous_job_id: previous.previous_job_id.clone(),
                signature,
            };
            previous.previous_job_id = job.id.clone();
            Some(evidence)
        }
        _ => {
            memory.insert(
                worker_key,
                ReviewLoopFailureMemory {
                    signature,
                    first_job_id: job.id.clone(),
                    previous_job_id: job.id.clone(),
                    repeat_count: 1,
                },
            );
            None
        }
    }
}

fn review_loop_recovery_delay_ms(
    config: &RuntimeConfig,
    job: &ReviewJob,
    repeat_count: usize,
) -> Option<u64> {
    let diagnostic = gemini_review_health_diagnostic(job)?;
    if !diagnostic.is_recoverable() {
        return None;
    }

    let base_delay = diagnostic
        .retry_after_ms
        .unwrap_or(config.polling.interval_ms)
        .max(1);
    let multiplier = if diagnostic.retry_after_ms.is_some() {
        1
    } else {
        let exponent = repeat_count.saturating_sub(1).min(5) as u32;
        2u64.saturating_pow(exponent)
    };
    let cap = config.agent.max_retry_backoff_ms.max(1);
    Some(base_delay.saturating_mul(multiplier).min(cap).max(1))
}

pub(crate) fn review_loop(options: ReviewLoopOptions) -> Result<(), Box<dyn std::error::Error>> {
    let limit = options.iteration_limit();
    let mut iterations = 0usize;
    let mut failure_memory = BTreeMap::<String, ReviewLoopFailureMemory>::new();

    loop {
        if let Some(max) = limit {
            if iterations >= max {
                println!("review_loop=stopped reason=max_iterations iterations={iterations}");
                break;
            }
        }

        iterations += 1;
        let workflow = WorkflowDefinition::load(&options.workflow_path)?;
        let config = RuntimeConfig::from_workflow(&workflow, &options.workflow_path)?;
        config.validate()?;
        preflight_canonical_checkout_for_write_mode(&config, "review_loop", options.write)?;
        let adapter = adapter_from_config(&config);
        let issues = run_with_progress_heartbeat(
            progress_spec_with_event_log(&config, "github_project_read")
                .backend(tracker_backend_label(&config))
                .next("review_queue_scan"),
            || {
                adapter.fetch_issues_by_states(std::slice::from_ref(
                    &config.tracker.state_map.agent_review,
                ))
            },
        )?;
        let issues = run_with_progress_heartbeat(
            progress_spec_with_event_log(&config, "github_project_read")
                .backend(tracker_backend_label(&config))
                .next("review_hydrate_issues"),
            || hydrate_issues_for_review_lane(adapter.as_ref(), issues),
        )?;

        if issues.is_empty() {
            if let Some(delay_ms) = unbounded_loop_sleep_ms(limit, config.polling.interval_ms) {
                println!(
                    "review_loop_idle action=sleep reason=no_agent_review_issue delay_ms={delay_ms} iterations={iterations}"
                );
                thread::sleep(Duration::from_millis(delay_ms));
                continue;
            }
            println!("review_loop=stopped reason=no_agent_review_issue iterations={iterations}");
            break;
        };

        let backend_kind = review_backend_kind(&config, options.fake_outcome.as_ref());
        let selected = select_review_worker_issues(
            &issues,
            &config.tracker.state_map.agent_review,
            &backend_kind,
            options.worker_limit(&config),
        );

        if selected.is_empty() {
            for issue in issues {
                match review_run_eligibility(
                    &issue,
                    &config.tracker.state_map.agent_review,
                    &backend_kind,
                ) {
                    ReviewRunEligibility::AlreadyQueued { worker_key } => {
                        println!(
                            "review_loop_action=skip issue={} reason=review_worker_exists worker_key={worker_key}",
                            issue.identifier
                        );
                    }
                    ReviewRunEligibility::NotInAgentReview { current_state } => {
                        println!(
                            "review_loop_action=skip issue={} reason=state_changed current_state={current_state:?}",
                            issue.identifier
                        );
                    }
                    ReviewRunEligibility::InvalidHandoff { reason } => {
                        println!(
                            "review_loop_action=skip issue={} reason=invalid_handoff detail={reason:?}",
                            issue.identifier
                        );
                        record_review_invalid_handoff(
                            &config,
                            adapter.as_ref(),
                            &issue,
                            &reason,
                            options.write,
                        )?;
                    }
                    ReviewRunEligibility::Eligible { .. } => {}
                }
            }
            if let Some(delay_ms) = unbounded_loop_sleep_ms(limit, config.polling.interval_ms) {
                println!(
                    "review_loop_idle action=sleep reason=no_available_review_worker delay_ms={delay_ms} iterations={iterations}"
                );
                thread::sleep(Duration::from_millis(delay_ms));
            }
            continue;
        }

        let mut pending_review_jobs: Vec<(
            usize,
            TrackerIssue,
            LaneClaim,
            thread::JoinHandle<ReviewJob>,
        )> = Vec::new();

        for (slot, selected_issue) in selected.into_iter().enumerate() {
            let worker_slot = slot + 1;
            match review_run_eligibility(
                &selected_issue,
                &config.tracker.state_map.agent_review,
                &backend_kind,
            ) {
                ReviewRunEligibility::Eligible { worker_key } => {
                    print_latest_status(&latest_status_for_issue(
                        &config,
                        &selected_issue,
                        "review",
                        if options.write { "running" } else { "waiting" },
                        "review_selected",
                        Some("write review timeline and reconcile".into()),
                    ));
                    println!(
                    "review_loop_iteration={iterations} worker_slot={worker_slot} issue={} worker_key={worker_key} mode={}",
                    selected_issue.identifier,
                    if options.write { "write" } else { "dry-run" }
                );
                    if !options.write {
                        println!(
                            "review_loop_dry_run action=start issue={} backend={backend_kind} mode={}",
                            selected_issue.identifier,
                            if backend_kind == "gemini-cli" {
                                "headless"
                            } else {
                                "job"
                            }
                        );
                        if backend_kind == "gemini-cli" {
                            println!(
                                "review_loop_dry_run action=command issue={} command={} args={}",
                                selected_issue.identifier,
                                shell_quote_display(&config.review.gemini_command),
                                gemini_cli_headless_args(
                                    config.review.gemini_model.as_deref(),
                                    &config.review.gemini_allowed_tools,
                                )
                                .join(" ")
                            );
                        }
                        print_review_claim_field_dry_run(&selected_issue, &worker_key);
                        println!(
                            "review_loop_dry_run action=timeline_comment issue={} evidence=review_job",
                            selected_issue.identifier
                        );
                        println!(
                            "review_loop_dry_run action=reconcile issue={} actor=independent_review_agent",
                            selected_issue.identifier
                        );
                        continue;
                    }

                    let latest = run_with_progress_heartbeat(
                        progress_spec_with_event_log(&config, "github_project_read")
                            .issue(selected_issue.identifier.clone())
                            .backend(tracker_backend_label(&config))
                            .next("review_issue_read"),
                        || adapter.get_issue(&selected_issue.identifier),
                    )?
                    .ok_or_else(|| {
                        format!(
                            "issue disappeared before review: {}",
                            selected_issue.identifier
                        )
                    })?;
                    match review_run_eligibility(
                        &latest,
                        &config.tracker.state_map.agent_review,
                        &backend_kind,
                    ) {
                        ReviewRunEligibility::Eligible { worker_key } => {
                            let claim = write_review_claim_field(
                                &config,
                                adapter.as_ref(),
                                &latest,
                                &worker_key,
                            )?;
                            let workflow_for_job = workflow.clone();
                            let config_for_job = config.clone();
                            let issue_for_job = latest.clone();
                            let fake_outcome_for_job = options.fake_outcome.clone();
                            let backend_kind_for_job = backend_kind.clone();
                            println!(
                                "review_loop_action=start issue={} worker_slot={} backend={} mode={}",
                                latest.identifier,
                                worker_slot,
                                backend_kind,
                                if backend_kind == "gemini-cli" { "headless" } else { "job" }
                            );
                            let handle = thread::spawn(move || {
                                run_review_job(
                                    &workflow_for_job,
                                    &config_for_job,
                                    &issue_for_job,
                                    fake_outcome_for_job,
                                )
                                .unwrap_or_else(|error| {
                                    ReviewJob::failed_unavailable(
                                        issue_for_job.identifier.clone(),
                                        backend_kind_for_job,
                                        error.to_string(),
                                    )
                                })
                            });
                            pending_review_jobs.push((worker_slot, latest, claim, handle));
                        }
                        ReviewRunEligibility::AlreadyQueued { worker_key } => {
                            println!(
                            "review_loop_action=skip issue={} reason=review_worker_exists worker_key={worker_key}",
                            latest.identifier
                        );
                        }
                        ReviewRunEligibility::NotInAgentReview { current_state } => {
                            println!(
                            "review_loop_action=skip issue={} reason=state_changed current_state={current_state:?}",
                            latest.identifier
                        );
                        }
                        ReviewRunEligibility::InvalidHandoff { reason } => {
                            println!(
                            "review_loop_action=skip issue={} reason=invalid_handoff detail={reason:?}",
                            latest.identifier
                        );
                            record_review_invalid_handoff(
                                &config,
                                adapter.as_ref(),
                                &latest,
                                &reason,
                                options.write,
                            )?;
                        }
                    }
                }
                ReviewRunEligibility::AlreadyQueued { worker_key } => {
                    println!(
                    "review_loop_action=skip issue={} reason=review_worker_exists worker_key={worker_key}",
                    selected_issue.identifier
                );
                }
                ReviewRunEligibility::NotInAgentReview { current_state } => {
                    println!(
                    "review_loop_action=skip issue={} reason=state_changed current_state={current_state:?}",
                    selected_issue.identifier
                );
                }
                ReviewRunEligibility::InvalidHandoff { reason } => {
                    println!(
                        "review_loop_action=skip issue={} reason=invalid_handoff detail={reason:?}",
                        selected_issue.identifier
                    );
                    record_review_invalid_handoff(
                        &config,
                        adapter.as_ref(),
                        &selected_issue,
                        &reason,
                        options.write,
                    )?;
                }
            }
        }

        for (worker_slot, latest, claim, handle) in pending_review_jobs {
            let mut job = match handle.join() {
                Ok(job) => job,
                Err(_) => ReviewJob::failed_unavailable(
                    latest.identifier.clone(),
                    backend_kind.clone(),
                    "review worker thread panicked",
                ),
            };
            let ledger_path =
                write_review_job_ledger_record(&config.observability.logs_root, &latest, &job)?;
            job.ledger_path = Some(ledger_path.clone());
            let repeat_evidence =
                review_loop_repeated_failure_evidence(&mut failure_memory, &latest, &job);
            apply_review_result(
                &config,
                adapter.as_ref(),
                &latest.identifier,
                &latest,
                &job,
                Some(&claim),
                repeat_evidence.as_ref(),
            )?;
            let decision = review_gate_decision_for_issue(&job, &latest);
            println!(
                "review_loop_action=reconciled issue={} worker_slot={} backend={} outcome={:?} target_state={:?} ledger={}",
                latest.identifier,
                worker_slot,
                job.backend,
                decision.outcome,
                decision.target_state,
                ledger_path.display()
            );
            if let Some(diagnostic) = gemini_review_health_diagnostic(&job) {
                println!(
                    "review_loop_health issue={} category={} recovery_policy={} retry_after_ms={} repeat_count={}",
                    latest.identifier,
                    diagnostic.category.as_str(),
                    diagnostic.recovery_policy.as_str(),
                    diagnostic
                        .retry_after_ms
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".into()),
                    repeat_evidence
                        .as_ref()
                        .map(|evidence| evidence.repeat_count)
                        .unwrap_or(1)
                );
            }
            if !options.once {
                let repeat_count = repeat_evidence
                    .as_ref()
                    .map(|evidence| evidence.repeat_count)
                    .unwrap_or(1);
                if let Some(delay_ms) = review_loop_recovery_delay_ms(&config, &job, repeat_count) {
                    let policy = gemini_review_health_diagnostic(&job)
                        .map(|diagnostic| diagnostic.recovery_policy)
                        .unwrap_or(GeminiReviewRecoveryPolicy::RetryWithBackoff);
                    println!(
                        "review_loop_action=wait issue={} reason=gemini_backend_health policy={} delay_ms={} repeat_count={}",
                        latest.identifier,
                        policy.as_str(),
                        delay_ms,
                        repeat_count
                    );
                    thread::sleep(Duration::from_millis(delay_ms));
                }
            }
        }

        if !options.write {
            let Some(delay_ms) = unbounded_loop_sleep_ms(limit, config.polling.interval_ms) else {
                continue;
            };
            println!(
                "review_loop_idle action=sleep reason=dry_run_would_repeat_without_mutation delay_ms={delay_ms} iterations={iterations}"
            );
            thread::sleep(Duration::from_millis(delay_ms));
        }
    }

    Ok(())
}

pub(crate) fn review_backend_kind(
    config: &RuntimeConfig,
    fake_outcome: Option<&FakeReviewOutcome>,
) -> String {
    if fake_outcome.is_some() {
        "fake-reviewer".into()
    } else if config.review.backend == "gemini-cli" {
        "gemini-cli".into()
    } else {
        "fake-reviewer".into()
    }
}

fn record_review_invalid_handoff(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    reason: &str,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let workpad = [
        "## Jade Symphony Agent Review Run".to_string(),
        String::new(),
        "### Agent Review Invalid Handoff".to_string(),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        "- Lane: `review`".to_string(),
        "- Input state: `Agent Review`".to_string(),
        "- Target state after review routing: `unchanged`".to_string(),
        "- Actor role: `review_agent`".to_string(),
        "- Decision: `inconclusive_invalid_handoff`".to_string(),
        format!("- Reason: {reason}"),
        "- Review did not start because the Main Agent handoff invariant is not satisfied.".to_string(),
        "- Draft PRs must be marked ready by the Main Agent lane or an operator-confirmed doctor repair before normal Agent Review.".to_string(),
    ]
    .join("\n");

    if write {
        adapter.add_issue_comment(&issue.identifier, &workpad)?;
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "review loop",
                mutation_type: "timeline_comment",
                issue_ref: Some(&issue.identifier),
                target: Some("invalid_handoff".into()),
                from_state: Some(issue.state.clone()),
                to_state: Some(issue.state.clone()),
                reason: "review refused invalid Agent Review handoff",
            },
        );
    } else {
        println!(
            "review_loop_dry_run action=timeline_comment issue={} evidence=invalid_handoff",
            issue.identifier
        );
    }

    Ok(())
}

pub(crate) fn select_review_worker_issues(
    issues: &[TrackerIssue],
    agent_review_state: &str,
    backend_kind: &str,
    max_concurrent: usize,
) -> Vec<TrackerIssue> {
    issues
        .iter()
        .filter(|issue| {
            matches!(
                review_run_eligibility(issue, agent_review_state, backend_kind),
                ReviewRunEligibility::Eligible { .. }
            )
        })
        .take(max_concurrent.max(1))
        .cloned()
        .collect()
}

pub(crate) fn review_claim_for_issue(issue: &TrackerIssue, worker_key: &str) -> LaneClaim {
    lane_claim_for_issue(
        issue,
        LaneClaimLane::Review,
        if worker_key.to_ascii_lowercase().contains("gemini") {
            LaneClaimActor::Gemini
        } else {
            LaneClaimActor::Codex
        },
        LaneClaimSource::Loop,
        project_text_field(issue, "Review Agent").as_deref(),
    )
    .with_worker(worker_key)
}

fn print_review_claim_field_dry_run(issue: &TrackerIssue, worker_key: &str) {
    let claim = review_claim_for_issue(issue, worker_key);
    println!(
        "review_loop_dry_run action=claim_field issue={} field={:?} value={:?}",
        issue.identifier,
        "Review Agent",
        claim.render()
    );
}

fn write_review_claim_field(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    worker_key: &str,
) -> Result<LaneClaim, Box<dyn std::error::Error>> {
    let claim = review_claim_for_issue(issue, worker_key);
    let claim_value = render_parseable_lane_claim(&claim)?;
    let outcome = set_project_field_with_recovery(
        adapter,
        issue,
        &ProjectFieldAssignment {
            name: "Review Agent".into(),
            value: claim_value.clone(),
        },
        "claim_field",
    )?;
    if outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "review loop",
                mutation_type: "claim_field",
                issue_ref: Some(&issue.identifier),
                target: Some(format!("Review Agent={claim_value}")),
                from_state: None,
                to_state: None,
                reason: "review worker claim",
            },
        );
    }
    println!(
        "review_loop_action=claim_field issue={} field=\"Review Agent\" run={} outcome={}",
        issue.identifier,
        claim.run,
        outcome.as_str()
    );
    Ok(claim)
}

fn run_review_job(
    workflow: &WorkflowDefinition,
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    fake_outcome: Option<FakeReviewOutcome>,
) -> Result<ReviewJob, Box<dyn std::error::Error>> {
    let request = ReviewRequest {
        issue: issue.clone(),
        prompt: render_automatic_review_prompt(workflow, issue)?,
        workspace: review_workspace_for_issue(config, issue),
        artifact_root: config.observability.logs_root.join("reviews"),
    };

    if let Some(outcome) = fake_outcome {
        let backend = FakeReviewBackend::new(outcome);
        let job = backend.start(request)?;
        let spec = review_backend_progress_spec(config, issue, backend.kind(), &job);
        return Ok(run_with_progress_heartbeat(spec, || {
            poll_review_job_until_terminal(
                &backend,
                job,
                Duration::from_millis(config.review.timeout_ms),
                Duration::from_millis(250),
            )
        })?);
    }

    match config.review.backend.as_str() {
        "gemini-cli" => {
            if let Some(diagnostic) = gemini_prelaunch_health_diagnostic(
                &config.review.gemini_command,
                config.review.gemini_model.as_deref(),
                &config.review.gemini_allowed_tools,
            ) {
                return Ok(ReviewJob::failed_unavailable(
                    issue.identifier.clone(),
                    "gemini-cli",
                    diagnostic.to_error_message(),
                ));
            }
            let backend = GeminiCliReviewBackend::with_headless_options(
                config.review.gemini_command.clone(),
                config.review.gemini_model.clone(),
                config.review.gemini_allowed_tools.clone(),
            );
            match backend.start(request) {
                Ok(job) => {
                    let spec = review_backend_progress_spec(config, issue, backend.kind(), &job);
                    Ok(run_with_progress_heartbeat(spec, || {
                        poll_review_job_until_terminal(
                            &backend,
                            job,
                            Duration::from_millis(config.review.timeout_ms),
                            Duration::from_millis(500),
                        )
                    })?)
                }
                Err(error) => Ok(ReviewJob::failed_unavailable(
                    issue.identifier.clone(),
                    "gemini-cli",
                    error.to_string(),
                )),
            }
        }
        _ => {
            let backend = FakeReviewBackend::new(FakeReviewOutcome::Pass);
            Ok(backend.poll(backend.start(request)?)?)
        }
    }
}

fn review_backend_progress_spec(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    backend: &str,
    job: &ReviewJob,
) -> ProgressHeartbeatSpec {
    let mut spec = progress_spec_with_event_log(config, "review_backend")
        .issue(issue.identifier.clone())
        .backend(backend)
        .next("waiting_for_child");
    if let Some(path) = &job.artifact_path {
        spec = spec.artifact(path.display().to_string());
    }
    spec
}

pub(crate) fn review_workspace_for_issue(config: &RuntimeConfig, issue: &TrackerIssue) -> PathBuf {
    if let Ok(repo_root) = std::env::current_dir() {
        if let Ok(report) = discover_issue_workspaces(config, issue, &repo_root) {
            if let Some(index) = report.canonical_index {
                if let Some(candidate) = report.candidates.get(index) {
                    if candidate.strength == WorkspaceMatchStrength::Strong
                        && candidate.path.starts_with(&config.workspace.root)
                    {
                        return candidate.path.clone();
                    }
                }
            }
        }
    }

    run_loop_handoff_plan(config, issue)
        .map(|handoff| handoff.workspace_path)
        .unwrap_or_else(|_| config.workspace.root.clone())
}

pub(crate) fn render_automatic_review_prompt(
    workflow: &WorkflowDefinition,
    issue: &TrackerIssue,
) -> Result<String, jade_symphony::prompt::PromptError> {
    let mut prompt = render_prompt(
        workflow.prompt_for_lane(AgentLane::ReviewAgent),
        issue,
        None,
    )?;
    prompt.push_str(
        "\n\n## Automatic Headless Review Boundary\n\n\
This Gemini process is running under Jade Symphony automatic `review loop` or `review once`.\n\
Jade Symphony CLI has already claimed or will own any Review Agent claim, timeline comment write,\n\
issue body update, and Project state transition outside this process.\n\n\
Do not run mutating Jade Symphony or GitHub commands, including `review claim`, `review pass`,\n\
`review reject`, `project set-state`, `project workpad`, `forge`, `gh issue edit`, `gh issue comment`, raw\n\
Project GraphQL mutations, or Project UI changes. Do not activate or follow any manual review\n\
skill that tells you to mutate Project state.\n\n\
Return review evidence in stdout only. Start with exactly one line: `Review Result: PASS`,\n\
`Review Result: REWORK`, or `Review Result: NEEDS_CONTEXT`. Use `PASS` only when there are no\n\
blocking findings. Use `REWORK` only when confirmed implementation defects require Main Agent\n\
changes. Use `NEEDS_CONTEXT` when missing evidence or ambiguity prevents an independent decision.\n\n\
UAT is Human Review-owned unless this issue explicitly asks the Main Agent to implement a UAT\n\
harness, fixture, rehearsal path, or workflow capability. Missing Human-owned UAT execution is\n\
not a confirmed implementation defect and must not by itself produce `Review Result: REWORK`.\n\
Report UAT readiness or Human Review follow-up separately under `Evidence`.\n\n\
Only use `[Confirmed]`, `[Plausible]`, `[Rejected]`, or `[Needs Context]` for actual review\n\
findings. Do not use those bracketed finding tags for positive verification evidence, checklist\n\
items, or things that were implemented correctly; put positive observations under an `Evidence`\n\
heading with plain bullets instead. Leave routing and evidence persistence to the Jade Symphony\n\
wrapper after this process exits.\n",
    );
    Ok(prompt)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReviewLoopOptions {
    pub(crate) workflow_path: PathBuf,
    pub(crate) max_iterations: Option<usize>,
    pub(crate) once: bool,
    pub(crate) write: bool,
    pub(crate) fake_outcome: Option<FakeReviewOutcome>,
    pub(crate) max_concurrent: Option<usize>,
}

impl ReviewLoopOptions {
    pub(crate) fn iteration_limit(&self) -> Option<usize> {
        if self.once {
            Some(1)
        } else {
            self.max_iterations
        }
    }

    pub(crate) fn worker_limit(&self, config: &RuntimeConfig) -> usize {
        self.max_concurrent
            .unwrap_or(config.review.max_concurrent_workers)
            .max(1)
    }
}

pub(crate) fn apply_review_result(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    issue: &TrackerIssue,
    job: &jade_symphony::review::ReviewJob,
    claim: Option<&LaneClaim>,
    repeat_evidence: Option<&ReviewRepeatedFailureEvidence>,
) -> Result<(), Box<dyn std::error::Error>> {
    let decision = review_gate_decision_for_issue(job, issue);
    if let Some(value) = terminal_review_loop_claim_value(claim, job, &decision) {
        write_terminal_review_claim(
            config,
            adapter,
            issue_ref,
            &issue.state,
            &value,
            "review loop terminal claim evidence",
        )?;
    }
    if decision.outcome.is_passed() {
        update_review_checklist_for_pass(
            config,
            adapter,
            issue,
            decision.target_state.unwrap_or("none"),
        )?;
    }
    if let Some(target_state) = decision.target_state {
        if !transition_allowed_for_review_agent(target_state, &decision) {
            return Err("review agent transition is not allowed for this review decision".into());
        }
        if rework_transition_expected(&decision) {
            transition_review_issue_to_rework_with_workpad(config, adapter, issue, job)?;
            return Ok(());
        }
    }

    let workpad = repeat_evidence
        .map(|evidence| render_repeated_review_failure_workpad(issue, job, evidence))
        .unwrap_or_else(|| render_review_workpad(issue, job));
    let evidence_key = recovery_key(
        "review-result",
        issue_ref,
        &format!(
            "{}|{:?}|{}|{}",
            issue_ref,
            decision.outcome,
            decision.target_state.unwrap_or("none"),
            job.ledger_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| stable_recovery_hash(&workpad))
        ),
    );
    let evidence_outcome = add_timeline_comment_with_recovery(
        adapter,
        issue_ref,
        Some(issue),
        &workpad,
        &evidence_key,
        "timeline_comment",
    )?;
    if evidence_outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "review loop",
                mutation_type: "timeline_comment",
                issue_ref: Some(issue_ref),
                target: job
                    .ledger_path
                    .as_ref()
                    .map(|path| path.display().to_string()),
                from_state: Some(issue.state.clone()),
                to_state: decision.target_state.map(ToOwned::to_owned),
                reason: "review result timeline evidence",
            },
        );
    }
    if let Some(target_state) = decision.target_state {
        let state_outcome = set_state_with_recovery(
            adapter,
            issue_ref,
            Some(issue),
            target_state,
            "state_change",
        )?;
        if state_outcome.should_record_audit() {
            append_tracker_mutation_audit(
                config,
                TrackerMutationAudit {
                    command: "review loop",
                    mutation_type: "state_change",
                    issue_ref: Some(issue_ref),
                    target: None,
                    from_state: Some(issue.state.clone()),
                    to_state: Some(target_state.into()),
                    reason: "review result routing",
                },
            );
        }
    }
    Ok(())
}

pub(crate) fn update_review_checklist_for_pass(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    target_state: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(description) = issue.description.as_deref() else {
        return Ok(());
    };
    let body = canonical_issue_body_without_workpad(description);
    let updated = check_review_verified_issue_body_checkboxes(&body);
    if updated == body {
        return Ok(());
    }

    adapter.update_issue_content(&issue.identifier, &issue.title, &updated)?;
    append_tracker_mutation_audit(
        config,
        TrackerMutationAudit {
            command: "review loop",
            mutation_type: "issue_body_update",
            issue_ref: Some(&issue.identifier),
            target: Some("non-UAT review checkboxes".into()),
            from_state: Some(issue.state.clone()),
            to_state: Some(target_state.into()),
            reason: "automatic review pass checklist evidence",
        },
    );
    Ok(())
}

pub(crate) fn canonical_issue_body_without_workpad(description: &str) -> String {
    description
        .split("<!-- jade-symphony-workpad -->")
        .next()
        .unwrap_or(description)
        .trim_end()
        .to_string()
}

pub(crate) fn check_review_verified_issue_body_checkboxes(body: &str) -> String {
    let mut in_fence = false;
    let mut in_review_section = false;
    let mut lines = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            lines.push(line.to_string());
            continue;
        }
        if !in_fence {
            if let Some(section) = markdown_heading_title(trimmed) {
                in_review_section = review_checklist_section_is_agent_owned(section);
            }
        }
        if in_review_section && !in_fence {
            lines.push(check_markdown_checkbox_line(line));
        } else {
            lines.push(line.to_string());
        }
    }
    let mut updated = lines.join("\n");
    if body.ends_with('\n') {
        updated.push('\n');
    }
    updated
}

fn markdown_heading_title(line: &str) -> Option<&str> {
    let heading_len = line.chars().take_while(|ch| *ch == '#').count();
    if heading_len == 0 || heading_len > 6 {
        return None;
    }
    if !line
        .chars()
        .nth(heading_len)
        .is_some_and(char::is_whitespace)
    {
        return None;
    }
    Some(line[heading_len..].trim().trim_matches('#').trim())
}

fn review_checklist_section_is_agent_owned(section: &str) -> bool {
    matches!(
        section.to_ascii_lowercase().as_str(),
        "expected outcome"
            | "completion criteria"
            | "functional verification"
            | "context verification"
    )
}

fn check_markdown_checkbox_line(line: &str) -> String {
    let trimmed = line.trim_start();
    if !(trimmed.starts_with("- [ ]") || trimmed.starts_with("* [ ]")) {
        return line.to_string();
    }
    if let Some(index) = line.find("[ ]") {
        let mut checked = line.to_string();
        checked.replace_range(index..index + 3, "[x]");
        checked
    } else {
        line.to_string()
    }
}

pub(crate) fn terminal_review_loop_claim_value(
    claim: Option<&LaneClaim>,
    job: &jade_symphony::review::ReviewJob,
    decision: &ReviewGateDecision,
) -> Option<String> {
    let claim = claim?;
    let (state, result) = match decision.outcome {
        ReviewOutcome::PassedToHumanReview | ReviewOutcome::PassedToMerging => {
            (LaneClaimState::Done, "passed")
        }
        ReviewOutcome::NeedsRework => (LaneClaimState::Done, "rejected"),
        ReviewOutcome::InconclusiveNeedsRework => (LaneClaimState::Failed, "inconclusive"),
        ReviewOutcome::NeedsHumanInput => (LaneClaimState::Failed, "blocked"),
        ReviewOutcome::BackendUnavailable => (LaneClaimState::Failed, "unavailable"),
        ReviewOutcome::Cancelled => (LaneClaimState::Failed, "cancelled"),
        ReviewOutcome::StillRunning => match job.state {
            ReviewJobState::Failed | ReviewJobState::TimedOut => {
                (LaneClaimState::Failed, "unavailable")
            }
            ReviewJobState::Cancelled => (LaneClaimState::Failed, "cancelled"),
            ReviewJobState::Queued | ReviewJobState::Running | ReviewJobState::Completed => {
                return None;
            }
        },
    };
    Some(terminal_review_claim_value(claim, state, result))
}

#[cfg(test)]
pub(crate) fn transition_issue_to_rework_with_diagnostic(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    diagnostic: &ReworkDiagnostic,
) -> Result<(), Box<dyn std::error::Error>> {
    let workpad = render_rework_diagnostic_workpad(issue, diagnostic);
    adapter.add_issue_comment(&issue.identifier, &workpad)?;
    append_tracker_mutation_audit(
        config,
        TrackerMutationAudit {
            command: "review loop",
            mutation_type: "timeline_comment",
            issue_ref: Some(&issue.identifier),
            target: diagnostic.review_ledger_path.clone(),
            from_state: Some(issue.state.clone()),
            to_state: Some("rework".into()),
            reason: "review rework diagnostic",
        },
    );
    adapter.set_state(&issue.identifier, "rework")?;
    append_tracker_mutation_audit(
        config,
        TrackerMutationAudit {
            command: "review loop",
            mutation_type: "state_change",
            issue_ref: Some(&issue.identifier),
            target: None,
            from_state: Some(issue.state.clone()),
            to_state: Some("rework".into()),
            reason: "confirmed review finding",
        },
    );
    Ok(())
}

fn transition_review_issue_to_rework_with_workpad(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    job: &ReviewJob,
) -> Result<(), Box<dyn std::error::Error>> {
    let workpad = render_review_workpad(issue, job);
    let evidence_key = recovery_key(
        "review-rework",
        &issue.identifier,
        &format!(
            "{}|{}",
            issue.identifier,
            job.ledger_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| stable_recovery_hash(&workpad))
        ),
    );
    let evidence_outcome = add_timeline_comment_with_recovery(
        adapter,
        &issue.identifier,
        Some(issue),
        &workpad,
        &evidence_key,
        "timeline_comment",
    )?;
    if evidence_outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "review loop",
                mutation_type: "timeline_comment",
                issue_ref: Some(&issue.identifier),
                target: job
                    .ledger_path
                    .as_ref()
                    .map(|path| path.display().to_string()),
                from_state: Some(issue.state.clone()),
                to_state: Some("rework".into()),
                reason: "review result timeline evidence",
            },
        );
    }
    let state_outcome = set_state_with_recovery(
        adapter,
        &issue.identifier,
        Some(issue),
        "rework",
        "state_change",
    )?;
    if state_outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "review loop",
                mutation_type: "state_change",
                issue_ref: Some(&issue.identifier),
                target: None,
                from_state: Some(issue.state.clone()),
                to_state: Some("rework".into()),
                reason: "confirmed review finding",
            },
        );
    }
    Ok(())
}
